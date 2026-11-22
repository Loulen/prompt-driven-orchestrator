import type { UpdateAttempt } from "../../shared/generated/UpdateAttempt";
import type { UpdateStatus } from "./update";

/**
 * The in-app update flow (#699), as a pure state machine so the UI's waiting /
 * reconnection / reload behaviour is testable without a daemon.
 *
 *   idle ─confirm+202─▶ applying ─socket lost─▶ restarting ─socket back─▶ verifying
 *     ▲                    │                           │                  │
 *     │                    │ attempt failed            │ no answer within │ version changed → reload
 *     │                    │ (daemon still up)         │ the bound        │ same version → back on same
 *     │                    ▼                           ▼                  │
 *     └──── dismiss ──── failed      rolled-back ◀── rolling-back         │
 *                          ▲                                              │
 *                          └──────────────────────────────────────────────┘
 *
 * The observable success is the NEW VERSION after reconnection (CONTEXT.md § *Mise à
 * jour depuis l'app*): a `/sessions` (or `/update`) answer whose version differs from
 * the one recorded at apply time triggers a page reload; the same version after a
 * restart means the binary did not change (failed upgrade, service came back on the
 * old file) and is reported as such, never as success.
 *
 * The Retour automatique (CORE-314): once the daemon has been gone longer than the
 * executor's verification bound, the executor is putting the previous version back
 * (`rolling-back`); the restored daemon then reports the attempt `failed` + rolled
 * back, and the flow ends on the « back on vY » card (`rolled-back`). While the
 * attempt still says `running`, the old version answering is not a verdict yet.
 */
export type UpdateFlowPhase =
  | "idle"
  | "applying"
  | "restarting"
  | "verifying"
  | "reload"
  | "rolling-back"
  | "failed"
  | "rolled-back"
  | "same-version";

export interface UpdateFlowState {
  phase: UpdateFlowPhase;
  /** The attempt spawned by the apply that started this flow. */
  attemptId: string | null;
  /** The version the daemon ran when the flow started. */
  fromVersion: string | null;
  /** Populated on `failed` / `same-version`. */
  error: string | null;
  /** Populated on `failed` / `same-version` when the daemon reported the attempt. */
  attempt: UpdateAttempt | null;
  /** Epoch ms when the flow started; drives the « taking longer than expected » hint. */
  startedAt: number;
  /** The version the update aims at, when known (the newest release, then the record's). */
  toVersion: string | null;
  /** The executor's verification bound, from the attempt record; `null` until read. */
  verifyTimeoutSecs: number | null;
  /** Epoch ms when the daemon went away (`restarting`); starts the verification clock. */
  restartedAt: number | null;
}

export type UpdateFlowEvent =
  | { type: "applied"; attemptId: string; fromVersion: string; toVersion?: string | null; now: number }
  | { type: "apply-failed"; error: string }
  /** A `GET /update` answered while the daemon is still up (polled during `applying`). */
  | { type: "status"; status: UpdateStatus }
  | { type: "socket"; connected: boolean; now?: number }
  /** The clock, while the daemon is away: past the verification bound, it is rolling back. */
  | { type: "tick"; now: number }
  /** The daemon answered `/sessions` (or `/update`) with this version after a reconnect. */
  | { type: "version"; version: string }
  | { type: "dismiss" };

export const IDLE: UpdateFlowState = {
  phase: "idle",
  attemptId: null,
  fromVersion: null,
  error: null,
  attempt: null,
  startedAt: 0,
  toVersion: null,
  verifyTimeoutSecs: null,
  restartedAt: null,
};

/** After this long without a result the waiting screen says so and links the log. */
export const UPDATE_SLOW_AFTER_MS = 90_000;

/**
 * When the « taking longer than expected » hint shows: past the slow threshold, plus
 * twice the verification bound when it is known (the wait for the new version, then
 * for the restored one).
 */
export function updateSlowAfterMs(state: UpdateFlowState): number {
  return UPDATE_SLOW_AFTER_MS + 2 * (state.verifyTimeoutSecs ?? 0) * 1000;
}

/** The phases in which the daemon may come back and decide the outcome. */
function awaitingVerdict(phase: UpdateFlowPhase): boolean {
  return phase === "applying" || phase === "restarting" || phase === "verifying" || phase === "rolling-back";
}

/** Why a failed attempt failed, in the words of its record. */
function failureReason(a: UpdateAttempt): string {
  return a.reason ?? `The update command exited with code ${a.exit_code ?? "?"}.`;
}

export function reduceUpdateFlow(state: UpdateFlowState, event: UpdateFlowEvent): UpdateFlowState {
  switch (event.type) {
    case "applied":
      return {
        phase: "applying",
        attemptId: event.attemptId,
        fromVersion: event.fromVersion,
        error: null,
        attempt: null,
        startedAt: event.now,
        toVersion: event.toVersion ?? null,
        verifyTimeoutSecs: null,
        restartedAt: null,
      };
    case "apply-failed":
      return { ...state, phase: "failed", error: event.error, attempt: null };
    case "status": {
      // A record about our attempt, from whichever daemon answers: the old one
      // still up (a failed command), or the restored one after a Retour automatique.
      if (!awaitingVerdict(state.phase)) return state;
      const a = event.status.last_attempt;
      if (!a || a.attempt_id !== state.attemptId) return state;
      if (a.status === "failed") {
        return {
          ...state,
          phase: a.rolled_back ? "rolled-back" : "failed",
          error: failureReason(a),
          attempt: a,
        };
      }
      // Unchanged news is no new state: the poll restarts on every new one.
      const toVersion = state.toVersion ?? a.to_version ?? null;
      const verifyTimeoutSecs = a.verify_timeout_secs ?? state.verifyTimeoutSecs;
      if (
        state.attempt?.status === a.status &&
        toVersion === state.toVersion &&
        verifyTimeoutSecs === state.verifyTimeoutSecs
      ) {
        return state;
      }
      return { ...state, attempt: a, toVersion, verifyTimeoutSecs };
    }
    case "socket": {
      if (state.phase === "applying" && !event.connected) {
        return { ...state, phase: "restarting", restartedAt: event.now ?? Date.now() };
      }
      if (state.phase === "restarting" && event.connected) {
        return { ...state, phase: "verifying" };
      }
      return state;
    }
    case "tick": {
      if (state.phase !== "restarting" || state.restartedAt == null || state.verifyTimeoutSecs == null) {
        return state;
      }
      return event.now - state.restartedAt > state.verifyTimeoutSecs * 1000
        ? { ...state, phase: "rolling-back" }
        : state;
    }
    case "version": {
      // A version seen while applying (socket never dropped, e.g. a very fast
      // restart) or while verifying decides the outcome.
      if (!awaitingVerdict(state.phase)) return state;
      if (state.fromVersion != null && event.version !== state.fromVersion) {
        return { ...state, phase: "reload" };
      }
      // The old version answering while the attempt still runs: the executor is
      // finishing a Retour automatique, its record decides.
      if (state.phase === "verifying" && state.attempt?.status !== "running") {
        return {
          ...state,
          phase: "same-version",
          error: `The daemon came back on v${event.version} — the binary did not change.`,
        };
      }
      return state;
    }
    case "dismiss":
      return IDLE;
  }
}

/** The waiting screen is up for every non-terminal phase after apply. */
export function isUpdateInProgress(state: UpdateFlowState): boolean {
  return awaitingVerdict(state.phase);
}

/** One line for the waiting screen, per phase. */
export function updateFlowMessage(state: UpdateFlowState): string {
  switch (state.phase) {
    case "applying":
      return "Running the update command…";
    case "restarting":
      return "The daemon is restarting — reconnecting…";
    case "verifying":
      return "Reconnected — checking the version…";
    case "reload":
      return "Updated — reloading the page…";
    case "rolling-back":
      return `Restoring v${state.fromVersion ?? "?"} and its data…`;
    default:
      return "";
  }
}

/** One step of the waiting screen: done, failed, or the one in progress. */
export interface UpdateFlowStep {
  label: string;
  state: "done" | "failed" | "active";
}

/**
 * The steps the waiting screen lists, per phase: the snapshot (taken before the
 * executor starts), the update command, then the restart — or, past the
 * verification bound, the new version that did not answer and the restore.
 */
export function updateFlowSteps(state: UpdateFlowState): UpdateFlowStep[] {
  const snapshot: UpdateFlowStep = { label: "Snapshot of the data and the binary", state: "done" };
  const ran: UpdateFlowStep = { label: "Update command ran", state: "done" };
  const active = (label: string): UpdateFlowStep => ({ label, state: "active" });
  switch (state.phase) {
    case "applying":
      return [snapshot, active(updateFlowMessage(state))];
    case "restarting":
    case "verifying":
      return [snapshot, ran, active(updateFlowMessage(state))];
    case "reload":
      return [snapshot, ran, { label: updateFlowMessage(state), state: "done" }];
    case "rolling-back":
      return [
        snapshot,
        ran,
        {
          label: `${state.toVersion ? `v${state.toVersion}` : "The new version"} did not answer within ${
            state.verifyTimeoutSecs ?? "?"
          } s`,
          state: "failed",
        },
        active(updateFlowMessage(state)),
      ];
    default:
      return [];
  }
}

/** Wording of the confirm dialog's Runs line, count-aware; never a block. */
export function activeRunsWarning(count: number): string {
  if (count <= 0) return "No active Run · the page reconnects on its own";
  const runs = count === 1 ? "1 active Run" : `${count} active Runs`;
  return `${runs} · tmux sessions survive, Runs resume with the daemon`;
}
