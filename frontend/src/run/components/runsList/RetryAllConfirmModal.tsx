import { ConfirmDialog } from "../../../shared/ui/kit";

/**
 * Retry all: the current Run is archived (it stays readable) and a fresh Run
 * of the same pipeline starts from the beginning.
 */
export default function RetryAllConfirmModal({
  onConfirm,
  onCancel,
}: {
  onConfirm: () => void;
  onCancel: () => void;
}) {
  return (
    <ConfirmDialog
      title="Retry all nodes?"
      consequence="Archives this run, starts a fresh one"
      actionLabel="Retry all"
      onConfirm={onConfirm}
      onClose={onCancel}
    />
  );
}
