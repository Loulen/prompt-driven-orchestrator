/**
 * The Run's Pipeline Manager tab: start it on demand, attach to its session, stop it.
 */

import { useState } from "react";
import { Terminal, Loader, Play, PowerOff } from "lucide-react";
import TmuxTerminal from "../../session/components/TmuxTerminal";
import { startRunManager, stopRunManager } from "../../run/api/manager";
import type { RunState } from "../../shared/generated/RunState";
import { useMayOnRun } from "../../run/store/gestureRightsStore";

export function ManagerTab({
  run,
  session,
  onRefreshRun,
  onOpenSettings,
}: {
  run: RunState;
  session: string;
  onRefreshRun?: () => void;
  onOpenSettings?: () => void;
}) {
  const hasManager = run.has_manager ?? false;
  // CORE-42: Start and Stop only when the caller may make them.
  const mayOnRun = useMayOnRun(run.run_id);
  const mayStart = mayOnRun("start_manager");
  const mayStop = mayOnRun("stop_manager");
  const [starting, setStarting] = useState(false);
  const [stopping, setStopping] = useState(false);
  const [confirmingStop, setConfirmingStop] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const start = async () => {
    setError(null);
    setStarting(true);
    try {
      // Idempotent on the daemon (a double-click is a benign re-answer). The
      // "starting" state holds until the refreshed run state flips
      // `has_manager` — the terminal mounts then, not here.
      await startRunManager(run.run_id);
      onRefreshRun?.();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setStarting(false);
    }
  };

  const stop = async () => {
    setStopping(true);
    try {
      await stopRunManager(run.run_id);
      onRefreshRun?.();
      setConfirmingStop(false);
      // Done with the transient state: once the refreshed run state lands, the
      // empty state — not a stale "starting…" — must be what the tab shows.
      setStarting(false);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setStopping(false);
    }
  };

  if (hasManager) {
    return (
      <div
        className="flex min-h-0 flex-1 flex-col"
        style={{ fontSize: "11.5px" }}
        data-testid="manager-live"
      >
        <div className="flex items-center gap-2 border-b border-line px-3 py-2">
          <Terminal size={14} className="shrink-0 text-text-3" />
          <div className="flex min-w-0 flex-1 flex-col">
            <span className="text-text-2" style={{ fontSize: "11px" }}>
              Pipeline Manager
            </span>
            <span
              className="truncate font-mono text-text-3"
              style={{ fontSize: "10px" }}
            >
              {session}
            </span>
          </div>
          {confirmingStop ? (
            <span className="flex shrink-0 items-center gap-1.5">
              <span className="text-text-3" style={{ fontSize: "10.5px" }}>
                Stop the manager?
              </span>
              <button
                onClick={() => void stop()}
                disabled={stopping}
                className="rounded border border-st-failed/40 bg-st-failed/14 px-2 py-0.5 text-st-failed transition-colors hover:border-st-failed disabled:opacity-40"
                style={{ fontSize: "10.5px" }}
                data-testid="manager-stop-confirm"
              >
                {stopping ? "Stopping…" : "Stop"}
              </button>
              <button
                onClick={() => setConfirmingStop(false)}
                className="rounded border border-line-strong bg-surface-2 px-2 py-0.5 text-text-3 transition-colors hover:text-text-2"
                style={{ fontSize: "10.5px" }}
                data-testid="manager-stop-cancel"
              >
                Cancel
              </button>
            </span>
          ) : (
            mayStop && <button
              onClick={() => setConfirmingStop(true)}
              className="flex shrink-0 items-center gap-1 rounded border border-line-strong bg-surface-2 px-2 py-1 text-text-3 transition-colors hover:border-st-failed hover:text-st-failed"
              style={{ fontSize: "10.5px" }}
              data-testid="manager-stop"
              title="Stop the manager session (survives until cleanup otherwise)"
            >
              <PowerOff size={11} />
              Stop
            </button>
          )}
        </div>
        {error && (
          <div
            className="border-b border-st-failed/30 bg-st-failed/14 px-3 py-1.5 text-st-failed"
            style={{ fontSize: "10.5px" }}
            role="alert"
            data-testid="manager-error"
          >
            {error}
          </div>
        )}
        <TmuxTerminal session={session} expanded status={run.status} />
      </div>
    );
  }

  if (starting) {
    return (
      <div
        className="flex flex-1 flex-col items-center justify-center gap-3 text-text-3"
        style={{ fontSize: "11.5px" }}
        data-testid="manager-starting"
      >
        <Loader size={18} className="animate-spin text-primary" />
        Starting the manager…
      </div>
    );
  }

  if (error) {
    return (
      <div
        className="flex flex-1 flex-col items-center justify-center gap-3 px-6 text-center"
        style={{ fontSize: "11.5px" }}
        data-testid="manager-start-error"
      >
        <div className="text-st-failed" role="alert">
          Failed to start the manager: {error}
        </div>
        <button
          onClick={() => void start()}
          className="rounded-md bg-primary px-3 py-1.5 font-medium text-primary-ink transition-colors hover:bg-primary-hi"
          style={{ fontSize: "11.5px" }}
          data-testid="manager-retry"
        >
          Retry
        </button>
      </div>
    );
  }

  return (
    <div
      className="flex flex-1 flex-col items-center justify-center gap-3 px-8 text-center"
      style={{ fontSize: "11.5px" }}
      data-testid="manager-empty-state"
    >
      <div className="grid h-11 w-11 place-items-center rounded-lg bg-surface-2 text-text-3">
        <Terminal size={18} />
      </div>
      <div className="font-medium text-text" style={{ fontSize: "13px" }}>
        No manager on this run
      </div>
      <p
        className="max-w-[260px] text-text-3"
        style={{ fontSize: "11.5px", lineHeight: 1.55 }}
      >
        The manager is a conversational agent that can drive this run — retry
        nodes, resolve merges, unblock it when it waits on you. It is off by
        default to keep costs down.
      </p>
      {mayStart && <button
        onClick={() => void start()}
        className="mt-1 flex items-center gap-1.5 rounded-md bg-primary px-3.5 py-2 font-medium text-primary-ink transition-colors hover:bg-primary-hi"
        style={{ fontSize: "12px" }}
        data-testid="manager-start"
      >
        <Play size={12} />
        Start manager
      </button>}
      <div className="text-text-3" style={{ fontSize: "10.5px" }}>
        or{" "}
        <button
          onClick={() => onOpenSettings?.()}
          className="cursor-pointer text-text-3 underline decoration-text-3 underline-offset-2 transition-colors hover:text-text"
          data-testid="manager-enable-settings"
        >
          enable it for every run in Settings
        </button>
      </div>
    </div>
  );
}

/**
 * The **Source** block of the Info tab (#803, ADR-0070 §4): where the Run was cut
 * from, its fork point, and how far the two have drifted apart since.
 *
 * Sits under Harness because it answers the same class of question — what this Run
 * is anchored to — and because the drift is what tells you, before you go near a
 * merge, whether the return will be a fast-forward or a conversation.
 *
 * The fetch button is the ONLY gesture here, and it is explicit: displaying a Run
 * never touches the network (ADR-0070 §1). It reuses the #802 verb, so a fetch run
 * from a Run refreshes exactly what the next launch form will read — same repository.
 * An archived Run whose branch was cleaned up loses it: there is nothing to refresh.
 */
