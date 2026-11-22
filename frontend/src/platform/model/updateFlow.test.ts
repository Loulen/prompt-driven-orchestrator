import { describe, it, expect } from "vitest";
import {
  IDLE,
  activeRunsWarning,
  isUpdateInProgress,
  reduceUpdateFlow,
  updateFlowMessage,
  updateFlowSteps,
  updateSlowAfterMs,
  UPDATE_SLOW_AFTER_MS,
  type UpdateFlowState,
} from "./updateFlow";
import type { UpdateAttempt } from "../../shared/generated/UpdateAttempt";
import type { UpdateStatus } from "./update";

// #699 — the waiting / reconnection / reload state machine behind the Update button.

function attempt(overrides: Partial<UpdateAttempt> = {}): UpdateAttempt {
  return { schema: "update-attempt-v1",
    attempt_id: "a1",
    status: "running",
    started_at: "2026-09-05T12:00:00Z",
    finished_at: null,
    exit_code: null,
    method: "homebrew",
    command: "brew update && brew upgrade Loulen/tap/pdo",
    supervision: "systemd",
    log_path: "/home/u/.pdo/update/a1.log",
    from_version: "1.60.0",
    to_version: "1.61.0",
    reason: null,
    rolled_back: false,
    verify_timeout_secs: 90,
    ...overrides,
  };
}

function status(last_attempt: UpdateAttempt | null, installed_version = "1.60.0"): UpdateStatus {
  return {
    channel_name: "PDO",
    installed_version,
    core_version: "1.60.0",
    latest_version: "1.61.0",
    newer_available: true,
    checked_at: null,
    source: "GitHub Releases",
    source_url: "",
    source_page_url: null,
    check_enabled: true,
    install_method: "homebrew",
    manual_command: "brew update && brew upgrade Loulen/tap/pdo",
    supervision: "systemd",
    reason: null,
    last_error: null,
    active_runs: 0,
    can_apply: last_attempt?.status !== "running",
    apply_blocked_reason: null,
    last_attempt,
  };
}

function applied(): UpdateFlowState {
  return reduceUpdateFlow(IDLE, { type: "applied", attemptId: "a1", fromVersion: "1.60.0", now: 1000 });
}

describe("reduceUpdateFlow", () => {
  it("apply → applying, with the attempt and the version to beat", () => {
    const s = applied();
    expect(s.phase).toBe("applying");
    expect(s.attemptId).toBe("a1");
    expect(s.fromVersion).toBe("1.60.0");
    expect(s.startedAt).toBe(1000);
    expect(isUpdateInProgress(s)).toBe(true);
    expect(updateFlowMessage(s)).toMatch(/Running the update/);
  });

  it("the happy path: socket lost → restarting, back → verifying, new version → reload", () => {
    let s = applied();
    s = reduceUpdateFlow(s, { type: "socket", connected: false });
    expect(s.phase).toBe("restarting");
    expect(updateFlowMessage(s)).toMatch(/restarting/);
    s = reduceUpdateFlow(s, { type: "socket", connected: true });
    expect(s.phase).toBe("verifying");
    s = reduceUpdateFlow(s, { type: "version", version: "1.61.0" });
    expect(s.phase).toBe("reload");
    expect(isUpdateInProgress(s)).toBe(false);
    expect(updateFlowMessage(s)).toMatch(/reloading/);
  });

  it("the same version after the restart is NOT a success", () => {
    let s = applied();
    s = reduceUpdateFlow(s, { type: "socket", connected: false });
    s = reduceUpdateFlow(s, { type: "socket", connected: true });
    s = reduceUpdateFlow(s, { type: "version", version: "1.60.0" });
    expect(s.phase).toBe("same-version");
    expect(s.error).toMatch(/did not change/);
  });

  it("a new version seen without a socket drop (fast restart) still reloads", () => {
    const s = reduceUpdateFlow(applied(), { type: "version", version: "1.61.0" });
    expect(s.phase).toBe("reload");
  });

  it("the same version while still applying is nothing yet (the old daemon answers)", () => {
    const s = reduceUpdateFlow(applied(), { type: "version", version: "1.60.0" });
    expect(s.phase).toBe("applying");
  });

  it("a failed attempt reported by the still-running daemon ends the flow with the record", () => {
    const s = reduceUpdateFlow(applied(), {
      type: "status",
      status: status(attempt({ status: "failed", exit_code: 3 })),
    });
    expect(s.phase).toBe("failed");
    expect(s.error).toBe("The update command exited with code 3.");
    expect(s.attempt?.attempt_id).toBe("a1");
  });

  it("a status about another attempt, or a running one, changes no phase", () => {
    const s = applied();
    expect(reduceUpdateFlow(s, { type: "status", status: status(attempt({ attempt_id: "zz", status: "failed" })) })).toBe(s);
    expect(reduceUpdateFlow(s, { type: "status", status: status(null) })).toBe(s);
    // A running record is read once: the target and the verification's bound.
    const read = reduceUpdateFlow(s, { type: "status", status: status(attempt()) });
    expect(read.phase).toBe("applying");
    expect(read.verifyTimeoutSecs).toBe(90);
    expect(read.toVersion).toBe("1.61.0");
    expect(reduceUpdateFlow(read, { type: "status", status: status(attempt()) })).toBe(read);
  });

  it("a failed command keeps the record's reason when it has one", () => {
    const s = reduceUpdateFlow(applied(), {
      type: "status",
      status: status(attempt({ status: "failed", exit_code: 3, reason: "The update command exited with code 3." })),
    });
    expect(s.phase).toBe("failed");
    expect(s.error).toBe("The update command exited with code 3.");
  });

  it("socket noise outside the flow is ignored", () => {
    expect(reduceUpdateFlow(IDLE, { type: "socket", connected: false })).toBe(IDLE);
    expect(reduceUpdateFlow(IDLE, { type: "version", version: "9.9.9" })).toBe(IDLE);
    // A reconnect while applying (never dropped) does not jump to verifying.
    const s = applied();
    expect(reduceUpdateFlow(s, { type: "socket", connected: true })).toBe(s);
  });

  it("apply-failed (409/500) ends the flow with the error; dismiss returns to idle", () => {
    const s = reduceUpdateFlow(IDLE, { type: "apply-failed", error: "Install method not detected" });
    expect(s.phase).toBe("failed");
    expect(s.error).toBe("Install method not detected");
    expect(reduceUpdateFlow(s, { type: "dismiss" })).toEqual(IDLE);
  });
});

describe("the Retour automatique (CORE-314)", () => {
  /** Applied, the running record read, then the daemon gone at t=10 s. */
  function away(): UpdateFlowState {
    let s = applied();
    s = reduceUpdateFlow(s, { type: "status", status: status(attempt()) });
    return reduceUpdateFlow(s, { type: "socket", connected: false, now: 10_000 });
  }

  it("past the verification bound without an answer, it is rolling back", () => {
    let s = away();
    expect(s.phase).toBe("restarting");
    expect(s.restartedAt).toBe(10_000);
    s = reduceUpdateFlow(s, { type: "tick", now: 10_000 + 90_000 });
    expect(s.phase).toBe("restarting");
    s = reduceUpdateFlow(s, { type: "tick", now: 10_000 + 91_000 });
    expect(s.phase).toBe("rolling-back");
    expect(isUpdateInProgress(s)).toBe(true);
    expect(updateFlowMessage(s)).toBe("Restoring v1.60.0 and its data…");
  });

  it("the steps: snapshot, command, the silent version, the restore", () => {
    expect(updateFlowSteps(applied())).toEqual([
      { label: "Snapshot of the data and the binary", state: "done" },
      { label: "Running the update command…", state: "active" },
    ]);
    const rolling = reduceUpdateFlow(away(), { type: "tick", now: 200_000 });
    expect(updateFlowSteps(rolling)).toEqual([
      { label: "Snapshot of the data and the binary", state: "done" },
      { label: "Update command ran", state: "done" },
      { label: "v1.61.0 did not answer within 90 s", state: "failed" },
      { label: "Restoring v1.60.0 and its data…", state: "active" },
    ]);
  });

  it("the restored daemon answering in the old version is no verdict while the attempt runs", () => {
    let s = reduceUpdateFlow(away(), { type: "socket", connected: true });
    expect(s.phase).toBe("verifying");
    s = reduceUpdateFlow(s, { type: "status", status: status(attempt()) });
    s = reduceUpdateFlow(s, { type: "version", version: "1.60.0" });
    expect(s.phase).toBe("verifying");
  });

  it("the record rolled back ends the flow on « back on vY » with its reason", () => {
    const reason =
      "v1.61.0 did not answer within 90 s after the restart. The previous version and its data were restored.";
    const rolling = reduceUpdateFlow(away(), { type: "tick", now: 200_000 });
    const s = reduceUpdateFlow(rolling, {
      type: "status",
      status: status(attempt({ status: "failed", exit_code: 1, rolled_back: true, reason })),
    });
    expect(s.phase).toBe("rolled-back");
    expect(s.error).toBe(reason);
    expect(isUpdateInProgress(s)).toBe(false);
  });

  it("the slow hint waits for the verification and the restore", () => {
    expect(updateSlowAfterMs(applied())).toBe(UPDATE_SLOW_AFTER_MS);
    expect(updateSlowAfterMs(away())).toBe(UPDATE_SLOW_AFTER_MS + 180_000);
  });
});

describe("activeRunsWarning", () => {
  it("counts the Runs and promises tmux survival, never a block", () => {
    expect(activeRunsWarning(0)).toMatch(/No active Run/);
    expect(activeRunsWarning(1)).toMatch(/^1 active Run ·/);
    expect(activeRunsWarning(3)).toMatch(/^3 active Runs ·/);
    expect(activeRunsWarning(3)).toMatch(/tmux sessions survive/);
  });
});
