import { useEffect } from "react";
import { Download, TriangleAlert } from "lucide-react";
import type { UpdateStatus } from "../model/update";
import { activeRunsWarning } from "../model/updateFlow";

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
      ? "The daemon is stopped and relaunched with its current arguments."
      : `The service unit is reinstalled with the stable binary path, then the ${
          update.supervision === "systemd" ? "systemd service" : "launchd agent"
        } restarts.`;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-scrim"
      data-testid="update-confirm"
    >
      <div className="w-[420px] rounded-lg border border-line bg-surface-1 p-4 shadow-lg">
        <h3 className="flex items-center gap-2 font-medium text-text" style={{ fontSize: "13px" }}>
          <Download size={14} className="text-primary" />
          Update {update.channel_name} to {target}?
        </h3>
        <p className="mt-2 text-text-3" style={{ fontSize: "12px" }}>
          PDO runs the command of your install method in a process detached from the daemon:
        </p>
        <code
          className="mt-1.5 block truncate rounded bg-surface-2 px-2 py-1 font-mono text-text"
          style={{ fontSize: "11px" }}
          data-testid="update-confirm-command"
        >
          {update.manual_command}
        </code>
        <p className="mt-2 text-text-3" style={{ fontSize: "12px" }}>
          {restart}
        </p>
        <p
          className={`mt-2 flex items-start gap-1.5 rounded border px-2.5 py-1.5 ${
            update.active_runs > 0
              ? "border-st-await/50 bg-st-await/14 text-st-await"
              : "border-line bg-surface-0 text-text-3"
          }`}
          style={{ fontSize: "11.5px", lineHeight: 1.45 }}
          data-testid="update-confirm-runs"
        >
          {update.active_runs > 0 && <TriangleAlert size={13} className="mt-px shrink-0" />}
          <span>{activeRunsWarning(update.active_runs)}</span>
        </p>
        <div className="mt-4 flex justify-end gap-2">
          <button
            type="button"
            onClick={onCancel}
            className="rounded px-3 py-1.5 text-text-3 hover:bg-hover hover:text-text-2"
            style={{ fontSize: "12px" }}
            data-testid="update-confirm-cancel"
          >
            Cancel
          </button>
          <button
            type="button"
            onClick={onConfirm}
            disabled={busy}
            autoFocus
            className="flex items-center gap-1.5 rounded bg-primary px-3 py-1.5 font-medium text-surface-0 hover:bg-primary/90 disabled:opacity-50"
            style={{ fontSize: "12px" }}
            data-testid="update-confirm-ok"
          >
            <Download size={12} />
            {busy ? "Starting…" : "Update now"}
          </button>
        </div>
      </div>
    </div>
  );
}
