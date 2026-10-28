import type { OpenPipeline } from "../store/editState";
import { hasUnsavedWork } from "../store/editState";
import { Button, Modal } from "../../shared/ui/kit";

interface Props {
  open: boolean;
  /**
   * The tabs a mass-close / replace would discard (#342). Only the ones holding
   * unsaved work are named — a save-error or unresolved conflict counts, since
   * closing the tab would drop that too.
   */
  tabs: OpenPipeline[];
  onCancel: () => void;
  onConfirm: () => void;
}

/**
 * All-or-nothing confirmation shown before a close/replace would throw away
 * unsaved work (#342): the title, the consequence, the unsaved tabs in mono
 * (a long list scrolls in the body, the actions stay in view, #940), Cancel
 * and Close.
 *
 * Deliberately no Enter→confirm binding (unlike `DestroyLoopModal`): this is a
 * destructive action and the user may have just been typing — an accidental
 * Enter must not discard their work. Escape and the backdrop cancel.
 */
export default function ConfirmCloseTabsModal({ open, tabs, onCancel, onConfirm }: Props) {
  if (!open) return null;

  const unsaved = tabs.filter(hasUnsavedWork);
  const plural = unsaved.length > 1;

  return (
    <Modal
      size="s"
      title="Discard unsaved changes?"
      onClose={onCancel}
      data-testid="close-tabs-backdrop"
      actions={
        <>
          <Button onClick={onCancel} data-testid="close-tabs-cancel" data-autofocus="">
            Cancel
          </Button>
          <Button tone="danger" onClick={onConfirm} data-testid="close-tabs-confirm">
            Close {plural ? "tabs" : "tab"} anyway
          </Button>
        </>
      }
    >
      <div className="d-confirm" data-testid="close-tabs-body">
        <p className="d-confirm__line">
          {plural ? `${unsaved.length} tabs lose their changes` : "Loses its changes"}
        </p>
        {unsaved.map((tab) => (
          <code key={tab.id} className="d-code d-confirm__target" title={`${tab.id}.yaml`}>
            {tab.id}.yaml
          </code>
        ))}
      </div>
    </Modal>
  );
}
