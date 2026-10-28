import { Button, Modal } from "../../../shared/ui/kit";
import { SETTINGS_CATEGORIES, type DirtyRollup } from "../../model/settingsCategories";

/** Closing Settings with an unsaved draft: keep editing, discard it, or save and close. */
export function ConfirmCloseDialog({
  rollup,
  saving,
  onKeep,
  onDiscard,
  onSaveAndClose,
}: {
  rollup: DirtyRollup;
  saving: boolean;
  onKeep: () => void;
  onDiscard: () => void;
  onSaveAndClose: () => void;
}) {
  const places = SETTINGS_CATEGORIES.flatMap((category) =>
    category.sections
      .filter((section) => rollup.sections.has(section.id))
      .map((section) => `${category.label} › ${section.label}`),
  );
  const count = rollup.fields.size;

  return (
    <Modal
      size="s"
      title="Close settings?"
      onClose={onKeep}
      onSubmit={onSaveAndClose}
      data-testid="settings-confirm-close"
      actions={
        <>
          <Button onClick={onKeep} data-testid="settings-confirm-keep" data-autofocus="">
            Keep editing
          </Button>
          <Button tone="danger" onClick={onDiscard} data-testid="settings-confirm-discard">
            Discard
          </Button>
          <Button tone="primary" onClick={onSaveAndClose} disabled={saving} data-testid="settings-confirm-save-close">
            {saving ? "Saving…" : "Save & close"}
          </Button>
        </>
      }
    >
      <div className="d-confirm">
        <p className="d-confirm__line">
          {count} unsaved change{count === 1 ? "" : "s"}
        </p>
        <code className="d-code d-confirm__target" data-testid="settings-confirm-places">
          {places.join(", ")}
        </code>
      </div>
    </Modal>
  );
}
