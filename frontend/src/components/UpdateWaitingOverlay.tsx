import { useEffect, useState } from "react";
import { Loader2, TriangleAlert } from "lucide-react";
import {
  UPDATE_SLOW_AFTER_MS,
  isUpdateInProgress,
  updateFlowMessage,
  type UpdateFlowState,
} from "../lib/updateFlow";

/**
 * The waiting screen during an in-app update (#699): covers the app from apply to
 * reload. Phase-aware message (running the command → daemon restarting → reconnected,
 * checking the version → reloading); after `UPDATE_SLOW_AFTER_MS` it says so and
 * points at Settings › Version & update where the attempt's log lives. Terminal
 * failures (`failed`, `same-version`) render as a dismissable card with the reason
 * — a failed update is never silent.
 */
export default function UpdateWaitingOverlay({
  flow,
  onDismiss,
  onOpenVersionSettings,
}: {
  flow: UpdateFlowState;
  onDismiss: () => void;
  onOpenVersionSettings: () => void;
}) {
  const [tick, setTick] = useState(() => Date.now());
  useEffect(() => {
    if (!isUpdateInProgress(flow)) return;
    const id = window.setInterval(() => setTick(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, [flow]);

  if (flow.phase === "idle") return null;

  const terminalFailure = flow.phase === "failed" || flow.phase === "same-version";
  const slow = isUpdateInProgress(flow) && tick - flow.startedAt > UPDATE_SLOW_AFTER_MS;

  return (
    <div
      className="fixed inset-0 z-[60] flex items-center justify-center bg-bg-0/80 backdrop-blur-sm"
      data-testid="update-waiting"
      data-phase={flow.phase}
    >
      <div className="w-[400px] rounded-lg border border-line bg-bg-2 p-5 shadow-lg">
        {terminalFailure ? (
          <>
            <h3 className="flex items-center gap-2 font-medium text-st-failed" style={{ fontSize: "13px" }}>
              <TriangleAlert size={14} />
              {flow.phase === "same-version" ? "Update did not take effect" : "Update failed"}
            </h3>
            <p className="mt-2 text-fg-3" style={{ fontSize: "12px" }} data-testid="update-waiting-error">
              {flow.error}
            </p>
            {flow.attempt && (
              <p className="mt-1.5 font-mono text-fg-4" style={{ fontSize: "11px" }}>
                attempt {flow.attempt.attempt_id} · {flow.attempt.command}
              </p>
            )}
            <p className="mt-2 text-fg-4" style={{ fontSize: "11.5px" }}>
              The attempt's log is in Settings › Version & update.
            </p>
            <div className="mt-4 flex justify-end gap-2">
              <button
                type="button"
                onClick={onOpenVersionSettings}
                className="rounded px-3 py-1.5 text-fg-3 hover:bg-bg-3 hover:text-fg-2"
                style={{ fontSize: "12px" }}
                data-testid="update-waiting-open-settings"
              >
                Open the log
              </button>
              <button
                type="button"
                onClick={onDismiss}
                autoFocus
                className="rounded border border-line-strong bg-bg-3 px-3 py-1.5 text-fg-2 hover:border-acc"
                style={{ fontSize: "12px" }}
                data-testid="update-waiting-dismiss"
              >
                Close
              </button>
            </div>
          </>
        ) : (
          <>
            <h3 className="flex items-center gap-2 font-medium text-fg" style={{ fontSize: "13px" }}>
              <Loader2 size={14} className="animate-spin text-acc" />
              Updating PDO{flow.fromVersion ? ` from v${flow.fromVersion}` : ""}…
            </h3>
            <p className="mt-2 text-fg-3" style={{ fontSize: "12px" }} data-testid="update-waiting-message">
              {updateFlowMessage(flow)}
            </p>
            <p className="mt-1.5 text-fg-4" style={{ fontSize: "11.5px" }}>
              The page reloads on its own once the daemon answers with the new version. Agent
              tmux sessions keep running meanwhile.
            </p>
            {slow && (
              <p
                className="mt-2 flex items-start gap-1.5 rounded border border-st-await/50 bg-st-await-bg px-2.5 py-1.5 text-st-await"
                style={{ fontSize: "11.5px" }}
                data-testid="update-waiting-slow"
              >
                <TriangleAlert size={13} className="mt-px shrink-0" />
                <span>
                  This is taking longer than expected. If the daemon does not come back, restart
                  it by hand and read the attempt's log in Settings › Version & update.
                </span>
              </p>
            )}
            {slow && (
              <div className="mt-3 flex justify-end">
                <button
                  type="button"
                  onClick={onDismiss}
                  className="rounded px-3 py-1.5 text-fg-3 hover:bg-bg-3 hover:text-fg-2"
                  style={{ fontSize: "12px" }}
                  data-testid="update-waiting-dismiss"
                >
                  Keep using the old version
                </button>
              </div>
            )}
          </>
        )}
      </div>
    </div>
  );
}
