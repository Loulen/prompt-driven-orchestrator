import { useId } from "react";
import { CalendarClock, Play, Save } from "lucide-react";
import { Button, Kbd, Tooltip } from "../../../shared/ui/kit";
import { baseName } from "../../../lib/homePath";
import type { NewRunForm } from "../../store/newRun/useNewRunForm";

/**
 * Left of the footer: the close warning when attachments would be lost, else
 * what launches, in mono (`4 nodes · claude · shop-api@main`, or the cron and
 * the pipeline of a Trigger).
 */
export function NewRunFooterNote({ form }: { form: NewRunForm }) {
  const {
    mode, attachments, closeWarning, attachmentsOverLimit, selectedPipeline, harness,
    instanceDefaultHarness, targetRepo, sourceBranch, resolvedCron, remoteUrl, cloneReused,
  } = form;
  if (closeWarning && attachments.length > 0) {
    return (
      <span className="ln-note ln-note--await" data-testid="attachments-close-warning">
        {attachments.length} attachment{attachments.length > 1 ? "s" : ""} lost on close
      </span>
    );
  }
  if (mode === "run" && attachmentsOverLimit) {
    return (
      <span className="ln-note ln-note--failed" data-testid="attachments-fix-hint">
        Attachments over the limit
      </span>
    );
  }
  if (remoteUrl) {
    return (
      <span className="ln-note" data-testid="clone-launch-note">
        {cloneReused ? "Existing clone" : "Clones first"}
      </span>
    );
  }
  const pipeline = selectedPipeline?.name ?? "no pipeline";
  const text =
    mode === "run"
      ? [
          selectedPipeline ? `${selectedPipeline.node_count} node${selectedPipeline.node_count === 1 ? "" : "s"}` : null,
          harness || instanceDefaultHarness || "claude",
          targetRepo.trim() ? `${baseName(targetRepo.trim())}@${sourceBranch || "HEAD"}` : null,
        ]
          .filter(Boolean)
          .join(" · ")
      : `${resolvedCron || "no schedule"} · ${pipeline}`;
  return (
    <span className="ln-note ln-mono" title={text} data-testid="new-run-summary">
      {text}
    </span>
  );
}

/** Right of the footer: Cancel, and Run / Create / Save, or the clone of a URL. */
export default function NewRunFooter({ form }: { form: NewRunForm }) {
  const {
    mode, editingTriggerId, submitting, attachments, requestClose, handleLaunch, canLaunch,
    launchBlockReason, canCreateTrigger, handleCreateTrigger, remoteUrl, clone, cloning, cloneReused,
  } = form;
  const launchReasonId = useId();
  const kbd = <Kbd>Ctrl ↵</Kbd>;
  return (
    <>
      <Button onClick={requestClose} data-testid="new-run-cancel">
        Cancel
      </Button>
      {remoteUrl ? (
        <Button
          tone="primary"
          onClick={() => void clone(mode === "run")}
          disabled={submitting || cloning}
          data-testid="clone-launch-button"
        >
          {cloning
            ? "Cloning…"
            : mode === "run"
              ? cloneReused
                ? "Launch"
                : "Clone and launch"
              : cloneReused
                ? "Use the clone"
                : "Clone"}
        </Button>
      ) : mode === "run" ? (
        // A disabled button takes no focus nor hover: the reason hangs on a
        // wrapper, focusable only while provisioning blocks the launch.
        <Tooltip label={launchBlockReason ?? "Run"} kbd={launchBlockReason ? undefined : "Ctrl+Enter"} side="top">
          <span tabIndex={launchBlockReason ? 0 : undefined} className="ln-btnwrap" data-testid="launch-button-wrapper">
            <Button
              tone="primary"
              icon={Play}
              onClick={() => void handleLaunch()}
              disabled={submitting || !canLaunch}
              aria-describedby={launchBlockReason ? launchReasonId : undefined}
              data-testid="launch-button"
            >
              {submitting ? (attachments.length > 0 ? "Uploading…" : "Launching…") : "Run"}
              {!submitting && kbd}
            </Button>
            {launchBlockReason && (
              <span id={launchReasonId} className="d-sr">
                {launchBlockReason}
              </span>
            )}
          </span>
        </Tooltip>
      ) : (
        <Button
          tone="primary"
          icon={editingTriggerId ? Save : CalendarClock}
          onClick={handleCreateTrigger}
          disabled={submitting || !canCreateTrigger}
          data-testid={editingTriggerId ? "save-trigger-button" : "create-trigger-button"}
        >
          {editingTriggerId ? (submitting ? "Saving…" : "Save") : submitting ? "Creating…" : "Create"}
          {!submitting && kbd}
        </Button>
      )}
    </>
  );
}
