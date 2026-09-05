import type { UpdateAttempt, UpdateStatus } from "../types";

/**
 * The in-app update flow (#699), as a pure state machine so the UI's waiting /
 * reconnection / reload behaviour is testable without a daemon.
 *
 *   idle ─confirm+202─▶ applying ─socket lost─▶ restarting ─socket back─▶ verifying
 *     ▲                    │                                              │
 *     │                    │ attempt failed (daemon still up)             │ version changed → reload
 *     │                    ▼                                              │ same version → back on same
 *     └──── dismiss ──── failed ◀──────────────────────────────────────────┘
 *
 * The observable success is the NEW VERSION after reconnection (CONTEXT.md § *Mise à
 * jour depuis l'app*): a `/sessions` (or `/update`) answer whose version differs from
 * the one recorded at apply time triggers a page reload; the same version after a
 * restart means the binary did not change (failed upgrade, service came back on the
 * old file) and is reported as such, never as success.
 */
export type UpdateFlowPhase =
  | "idle"
  | "applying"
  | "restarting"
  | "verifying"
  | "reload"
  | "failed"
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
}

export type UpdateFlowEvent =
  | { type: "applied"; attemptId: string; fromVersion: string; now: number }
  | { type: "apply-failed"; error: string }
  /** A `GET /update` answered while the daemon is still up (polled during `applying`). */
  | { type: "status"; status: UpdateStatus }
  | { type: "socket"; connected: boolean }
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
};

/** After this long without a result the waiting screen says so and links the log. */
export const UPDATE_SLOW_AFTER_MS = 90_000;

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
      };
    case "apply-failed":
      return { ...state, phase: "failed", error: event.error, attempt: null };
    case "status": {
      // Only while the old daemon is still answering: a failed attempt (executor
      // exited non-zero before touching the daemon) ends the flow with the record.
      if (state.phase !== "applying") return state;
      const a = event.status.last_attempt;
      if (!a || a.attempt_id !== state.attemptId) return state;
      if (a.status === "failed") {
        return {
          ...state,
          phase: "failed",
          error: `The update command exited with code ${a.exit_code ?? "?"}.`,
          attempt: a,
        };
      }
      return state;
    }
    case "socket": {
      if (state.phase === "applying" && !event.connected) {
        return { ...state, phase: "restarting" };
      }
      if (state.phase === "restarting" && event.connected) {
        return { ...state, phase: "verifying" };
      }
      return state;
    }
    case "version": {
      // A version seen while applying (socket never dropped, e.g. a very fast
      // restart) or while verifying decides the outcome.
      if (state.phase !== "verifying" && state.phase !== "applying" && state.phase !== "restarting") {
        return state;
      }
      if (state.fromVersion != null && event.version !== state.fromVersion) {
        return { ...state, phase: "reload" };
      }
      if (state.phase === "verifying") {
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
  return state.phase === "applying" || state.phase === "restarting" || state.phase === "verifying";
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
    default:
      return "";
  }
}

/** Wording of the confirm dialog's Runs line, count-aware; never a block. */
export function activeRunsWarning(count: number): string {
  if (count <= 0) {
    return "No Run is active. The daemon restarts; the page reconnects on its own.";
  }
  const runs = count === 1 ? "1 Run is active" : `${count} Runs are active`;
  return `${runs}. The daemon restarts — the agents' tmux sessions survive and the Runs resume with it; only the connection to this page drops for a moment.`;
}
