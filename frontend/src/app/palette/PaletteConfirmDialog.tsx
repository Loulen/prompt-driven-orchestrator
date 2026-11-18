import { ConfirmDialog } from "../../shared/ui/kit";
import GestureConfirmDialog from "../../run/components/GestureConfirmDialog";
import type { PaletteConfirm } from "./paletteStore";

/**
 * The confirmation of a gesture picked in the palette, once it closed: a
 * node gesture's in the window the node panel opens (CORE-206), any other in
 * the kit's `ConfirmDialog`. Confirm settles it and sends the gesture.
 */
export default function PaletteConfirmDialog({ confirm, settle }: { confirm: PaletteConfirm; settle: () => void }) {
  const onConfirm = () => {
    settle();
    confirm.run();
  };
  if (confirm.gesture) {
    return (
      <GestureConfirmDialog
        confirmation={confirm.gesture}
        target={confirm.target}
        onClose={settle}
        onConfirm={onConfirm}
      />
    );
  }
  return (
    <ConfirmDialog
      title={confirm.title}
      consequence={confirm.consequence}
      target={confirm.target}
      actionLabel={confirm.actionLabel}
      danger={confirm.danger}
      onClose={settle}
      onConfirm={onConfirm}
    />
  );
}
