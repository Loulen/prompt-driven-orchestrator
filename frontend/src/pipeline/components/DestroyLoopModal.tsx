import { ConfirmDialog } from "../../shared/ui/kit";

interface Props {
  open: boolean;
  /** Ids of the bounded regions this edge deletion would destroy (#150). */
  loopIds: string[];
  onClose: () => void;
  /** Deletes the edge; hand back a promise for the dialog to wait for it. */
  onConfirm: () => void | Promise<unknown>;
}

/**
 * Confirmation shown when deleting an edge would remove a bounded region's
 * **last** cycle (ADR-0011 / #150), on the kit's `ConfirmDialog`. Deleting
 * that edge destroys the loop: its `loops:` entry, bound, and iteration state
 * go with it. On confirm the caller deletes the edge (the store drops the
 * destroyed `loops:` entries); on cancel nothing changes. Deleting a non-last
 * cycle edge never reaches this dialog. A long list of loops scrolls in the
 * dialog's body, the actions stay visible (#940).
 */
export default function DestroyLoopModal({ open, loopIds, onClose, onConfirm }: Props) {
  if (!open) return null;
  const plural = loopIds.length > 1;
  return (
    <ConfirmDialog
      title={plural ? "Destroy these loops?" : "Destroy this loop?"}
      consequence={plural ? "Their bounds and iteration state go" : "Its bound and iteration state go"}
      actionLabel={`Delete edge & destroy ${plural ? "loops" : "loop"}`}
      danger
      onConfirm={onConfirm}
      onClose={onClose}
      data-testid="destroy-loop-modal"
      confirmTestId="destroy-loop-confirm"
      cancelTestId="destroy-loop-cancel"
    >
      <div className="d-confirm__targets" data-testid="destroy-loop-body">
        {loopIds.map((id) => (
          <code key={id} className="d-code d-confirm__target">
            {id}
          </code>
        ))}
      </div>
    </ConfirmDialog>
  );
}
