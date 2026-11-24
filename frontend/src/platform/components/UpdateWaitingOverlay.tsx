import { useEffect, useState } from "react";
import { Check, LoaderCircle, TriangleAlert, X } from "lucide-react";
import { Button } from "../../shared/ui/kit";
import {
  isUpdateInProgress,
  updateFlowMessage,
  updateFlowSteps,
  updateSlowAfterMs,
  type UpdateFlowStep,
  type UpdateFlowState,
} from "../model/updateFlow";

/** One step of the waiting screen, with its mark: done, failed, or in progress. */
function Step({ step, testId }: { step: UpdateFlowStep; testId?: string }) {
  const Icon = step.state === "done" ? Check : step.state === "failed" ? X : LoaderCircle;
  return (
    <span data-tone={step.state} data-testid={testId}>
      <Icon
        size={13}
        strokeWidth={1.5}
        aria-hidden
        className={
          step.state === "done" ? "text-st-done" : step.state === "failed" ? "text-st-failed" : "d-spin text-primary"
        }
      />
      {step.label}
    </span>
  );
}

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

  const rolledBack = flow.phase === "rolled-back";
  const terminalFailure = flow.phase === "failed" || flow.phase === "same-version" || rolledBack;
  const slow = isUpdateInProgress(flow) && tick - flow.startedAt > updateSlowAfterMs(flow);
  const steps = updateFlowSteps(flow);
  const failureTitle = rolledBack
    ? `Update failed · back on v${flow.fromVersion ?? "?"}`
    : flow.phase === "same-version"
      ? "Update did not take effect"
      : "Update failed";

  return (
    <div className="tr-scrim z-[60]" data-testid="update-waiting" data-phase={flow.phase}>
      <div
        className={`tr-frame tr-bcard ${terminalFailure || slow ? "" : "tr-bcard--open"}`}
        role="alertdialog"
        aria-label={terminalFailure ? "Update failed" : "Updating"}
      >
        {terminalFailure ? (
          <>
            <div className="tr-head">
              <TriangleAlert size={14} strokeWidth={1.5} className="text-st-failed" aria-hidden />
              <h2 className="tr-bcard__title" data-testid="update-waiting-title">
                {failureTitle}
              </h2>
            </div>
            <pre className="tr-reason" data-testid="update-waiting-error">
              {/* One sentence per line: what failed, then what the Retour automatique did. */}
              {rolledBack ? flow.error?.replace(/\. /g, ".\n") : flow.error}
            </pre>
            {flow.attempt && !rolledBack && (
              <div className="d-meta truncate font-mono" title={flow.attempt.command}>
                attempt {flow.attempt.attempt_id} · {flow.attempt.command}
              </div>
            )}
            <div className="tr-foot">
              <span className="d-grow" />
              <Button
                onClick={onOpenVersionSettings}
                title="Settings › General › Version & update"
                data-testid="update-waiting-open-settings"
              >
                Open the log
              </Button>
              <Button tone="primary" onClick={onDismiss} autoFocus data-testid="update-waiting-dismiss">
                Close
              </Button>
            </div>
          </>
        ) : (
          <>
            <div className="tr-head">
              <LoaderCircle size={14} strokeWidth={1.5} className="d-spin text-primary" aria-hidden />
              <h2 className="tr-bcard__title">
                Updating{flow.fromVersion ? ` from v${flow.fromVersion}` : ""}…
              </h2>
            </div>
            <div className="tr-facts" data-testid="update-waiting-steps">
              {steps.map((step, i) => (
                <Step
                  key={step.label}
                  step={step}
                  testId={i === steps.length - 1 ? "update-waiting-message" : undefined}
                />
              ))}
              {steps.length === 0 && <span data-testid="update-waiting-message">{updateFlowMessage(flow)}</span>}
              <span className="d-meta">Reloads by itself · agent sessions keep running</span>
            </div>
            {slow && (
              <>
                <div className="tr-banner" data-testid="update-waiting-slow">
                  <TriangleAlert size={13} strokeWidth={1.5} aria-hidden />
                  <span>Taking longer than expected · restart the daemon by hand if it does not come back</span>
                </div>
                <div className="tr-foot">
                  <span className="d-grow" />
                  <Button onClick={onDismiss} data-testid="update-waiting-dismiss">
                    Keep using the old version
                  </Button>
                </div>
              </>
            )}
          </>
        )}
      </div>
    </div>
  );
}
