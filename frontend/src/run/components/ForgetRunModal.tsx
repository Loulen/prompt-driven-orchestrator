import { ConfirmDialog } from "../../shared/ui/kit";

interface Props {
  runId: string;
  /** The forget itself: the dialog waits for it, closes on success, keeps a failure on screen. */
  onConfirm: () => Promise<unknown>;
  onCancel: () => void;
}

/**
 * The confirmation of forgetting an archived Run, on the kit's
 * `ConfirmDialog`: its event log is deleted, the Run appears nowhere any more
 * and the manager loses its post-mortem context. It cannot be undone.
 */
export default function ForgetRunModal({ runId, onConfirm, onCancel }: Props) {
  return (
    <ConfirmDialog
      title="Forget this run permanently?"
      consequence="Deletes its event log, for good"
      target={runId}
      actionLabel="Forget"
      pendingLabel="Forgetting…"
      failedTitle="Forgetting failed"
      danger
      onConfirm={onConfirm}
      onClose={onCancel}
      data-testid="forget-run-modal"
      confirmTestId="forget-run-confirm"
    />
  );
}
