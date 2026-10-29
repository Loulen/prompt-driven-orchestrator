import { useEffect } from "react";
import { Download, RotateCw, TriangleAlert } from "lucide-react";
import { Button } from "../../shared/ui/kit";
import type { UpdateStatus } from "../model/update";
import { activeRunsWarning } from "../model/updateFlow";
import { channelLabel } from "../model/updateStatus";

/**
 * The confirm before an in-app update (#699). Says what will happen — the method's
 * exact command, the daemon restart, how many Runs are active and that their tmux
 * sessions survive — and asks once. Active Runs WARN, they never block (CONTEXT.md §
 * *Mise à jour depuis l'app*): the button stays enabled whatever the count.
 */
export default function UpdateConfirmModal({
  update,
  onConfirm,
  onCancel,
  busy = false,
}: {
  update: UpdateStatus;
  onConfirm: () => void;
  onCancel: () => void;
  busy?: boolean;
}) {
  useEffect(() => {
    function handleKey(e: KeyboardEvent) {
      if (e.key === "Escape") onCancel();
    }
    document.addEventListener("keydown", handleKey);
    return () => document.removeEventListener("keydown", handleKey);
  }, [onCancel]);

  const target = update.latest_version ? `v${update.latest_version}` : "the latest release";
  const restart =
    update.supervision === "none"
      ? "Daemon stopped and relaunched with its current arguments"
      : `${update.supervision === "systemd" ? "systemd service" : "launchd agent"} reinstalled and restarted`;
  const title = `Update ${channelLabel(update)} to ${target}?`;

  return (
    <div className="tr-scrim z-50" data-testid="update-confirm">
      <div className="tr-frame tr-bcard" role="dialog" aria-label={title}>
        <div className="tr-head">
          <Download size={14} strokeWidth={1.5} className="text-primary" aria-hidden />
          <h2 className="tr-bcard__title">{title}</h2>
        </div>
        <code className="tr-cmd" title="Runs detached from the daemon" data-testid="update-confirm-command">
          {update.manual_command}
        </code>
        <div className="tr-facts">
          <span>
            <RotateCw size={12} strokeWidth={1.5} aria-hidden />
            {restart}
          </span>
          <span
            data-tone={update.active_runs > 0 ? "await" : undefined}
            className={update.active_runs > 0 ? "text-st-await" : undefined}
            data-testid="update-confirm-runs"
          >
            {update.active_runs > 0 && <TriangleAlert size={12} strokeWidth={1.5} aria-hidden />}
            {activeRunsWarning(update.active_runs)}
          </span>
        </div>
        <div className="tr-foot">
          <span className="d-grow" />
          <Button onClick={onCancel} data-testid="update-confirm-cancel">
            Cancel
          </Button>
          <Button
            tone="primary"
            icon={Download}
            onClick={onConfirm}
            disabled={busy}
            autoFocus
            data-testid="update-confirm-ok"
          >
            {busy ? "Starting…" : "Update now"}
          </Button>
        </div>
      </div>
    </div>
  );
}
