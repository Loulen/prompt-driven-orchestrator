import { useId } from "react";
import { Clock, Save, Sparkles } from "lucide-react";
import { Tooltip, TooltipProvider } from "../../../shared/ui/tooltip";
import type { NewRunForm } from "../../store/newRun/useNewRunForm";

/** The footer: the close warning, Cancel, and Launch / Create / Save trigger. */
export default function NewRunFooter({ form }: { form: NewRunForm }) {
  const {
    mode, editingTriggerId, submitting, attachments, closeWarning, attachmentsOverLimit,
    requestClose, handleLaunch, canLaunch, launchBlockReason, canCreateTrigger,
    handleCreateTrigger,
  } = form;
  const launchReasonId = useId();
  return (
    <div className="flex items-center justify-end gap-2 border-t border-line px-4 py-3">
      {closeWarning && attachments.length > 0 && (
        <span
          className="mr-auto text-st-await"
          style={{ fontSize: "10.5px" }}
          data-testid="attachments-close-warning"
        >
          {attachments.length} attachment{attachments.length > 1 ? "s" : ""} will be lost
          when the dialog closes — Cancel again to confirm.
        </span>
      )}
      {mode === "run" && attachmentsOverLimit && (
        <span
          className="text-fg-4"
          style={{ fontSize: "10.5px" }}
          data-testid="attachments-fix-hint"
        >
          Fix attachments to create the run
        </span>
      )}
      <button
        onClick={requestClose}
        data-testid="new-run-cancel"
        className="rounded-md border border-line-strong bg-bg-3 px-3 py-1.5 text-fg-2 transition-colors hover:bg-bg-4"
        style={{ fontSize: "11.5px" }}
      >
        Cancel
      </button>
      {mode === "run" ? (
        // A disabled button takes no focus nor hover: the reason hangs on a
        // wrapper, focusable only while provisioning blocks the launch.
        <TooltipProvider>
          <Tooltip content={launchBlockReason ?? ""} side="top" disabled={!launchBlockReason}>
            <span
              tabIndex={launchBlockReason ? 0 : undefined}
              className="inline-flex rounded-md"
              data-testid="launch-button-wrapper"
            >
              <button
                onClick={() => void handleLaunch()}
                disabled={submitting || !canLaunch}
                aria-describedby={launchBlockReason ? launchReasonId : undefined}
                className="flex items-center gap-1.5 rounded-md bg-acc px-3 py-1.5 font-medium text-on-acc transition-colors hover:bg-acc-dim disabled:opacity-40"
                style={{ fontSize: "11.5px" }}
                data-testid="launch-button"
              >
                <Sparkles size={12} />
                {submitting ? (attachments.length > 0 ? "Uploading…" : "Launching…") : "Launch"}
              </button>
              {launchBlockReason && (
                <span id={launchReasonId} className="sr-only">
                  {launchBlockReason}
                </span>
              )}
            </span>
          </Tooltip>
        </TooltipProvider>
      ) : (
        <button
          onClick={handleCreateTrigger}
          disabled={submitting || !canCreateTrigger}
          className="flex items-center gap-1.5 rounded-md bg-acc px-3 py-1.5 font-medium text-on-acc transition-colors hover:bg-acc-dim disabled:opacity-40"
          style={{ fontSize: "11.5px" }}
          data-testid={editingTriggerId ? "save-trigger-button" : "create-trigger-button"}
        >
          {editingTriggerId ? <Save size={12} /> : <Clock size={12} />}
          {editingTriggerId
            ? submitting
              ? "Saving…"
              : "Save trigger"
            : submitting
              ? "Creating…"
              : "Create trigger"}
        </button>
      )}
    </div>
  );
}
