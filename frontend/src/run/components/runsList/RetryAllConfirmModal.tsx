import { ConfirmDialog } from "../../../shared/ui/kit";

/**
 * Retry all: the current Run is archived (it stays readable) and a fresh Run
 * of the same pipeline starts from the beginning. The confirmation waits for
 * the daemon: closed once the fresh Run exists, its refusal kept on screen.
 */
export default function RetryAllConfirmModal({
  onConfirm,
  onCancel,
}: {
  /** The retry itself: the dialog waits for it, closes on success, keeps a failure on screen. */
  onConfirm: () => Promise<unknown>;
  onCancel: () => void;
}) {
  return (
    <ConfirmDialog
      title="Retry all nodes?"
      consequence="Archives this run, starts a fresh one"
      actionLabel="Retry all"
      pendingLabel="Retrying…"
      failedTitle="Retry all failed"
      onConfirm={onConfirm}
      onClose={onCancel}
    />
  );
}
