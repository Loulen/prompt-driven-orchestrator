import { Info } from "lucide-react";
import { ConfirmDialog, IconButton } from "../../shared/ui/kit";
import type { GestureConfirmation } from "../model/nodeGestureConfirmation";

/**
 * The confirmation of a node gesture (CORE-206), the same from every surface:
 * the kit's `ConfirmDialog`, one line per consequence, the (i) of the gesture
 * at the end of the last one (its sentence in the kit's tooltip, on hover and
 * keyboard focus), then the node's name in mono. The focus starts on the
 * action: Enter confirms, Escape or the backdrop cancels.
 */
export default function GestureConfirmDialog({
  confirmation,
  target,
  onConfirm,
  onClose,
}: {
  confirmation: GestureConfirmation;
  /** The node's name, in mono. */
  target: string;
  onConfirm: () => void;
  onClose: () => void;
}) {
  const { title, consequences, danger, actionLabel, info } = confirmation;
  const last = consequences.length - 1;
  return (
    <ConfirmDialog title={title} actionLabel={actionLabel} danger={danger} onConfirm={onConfirm} onClose={onClose}>
      {consequences.map((line, i) => (
        <p key={line} className="d-confirm__line" data-testid="gesture-confirm-line">
          {line}
          {i === last && info && (
            <IconButton icon={Info} size="sm" label={info.label} description={info.description} data-testid="gesture-confirm-info" />
          )}
        </p>
      ))}
      <code className="d-code d-confirm__target">{target}</code>
    </ConfirmDialog>
  );
}
