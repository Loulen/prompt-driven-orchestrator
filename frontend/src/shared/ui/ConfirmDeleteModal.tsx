import { useState } from "react";
import { Button, Modal } from "./kit";

interface Props {
  open: boolean;
  onClose: () => void;
  /**
   * Widened from `() => void` so the modal can report whether the opt-in
   * cascade checkbox was ticked (#227). Existing zero-arg handlers stay
   * assignable; callers that don't pass `cascadeLabel` always receive `false`.
   */
  onConfirm: (cascade: boolean) => void;
  name: string;
  kind?: string;
  /** The consequence, in five words at most. */
  detail?: string;
  /**
   * When set, render an opt-in checkbox (default unchecked) with this label
   * under the target. Its state flows through `onConfirm(cascade)`. Absent ⇒ no
   * checkbox, `onConfirm(false)` (#227).
   */
  cascadeLabel?: string;
}

/**
 * The confirmation of a deletion: the title names the action and the kind of
 * thing, then the consequence in a few words, the target in mono, and Cancel /
 * Delete. Enter (outside a text field) or Ctrl+Enter deletes, Escape and the
 * backdrop cancel.
 */
export default function ConfirmDeleteModal({
  open,
  onClose,
  onConfirm,
  name,
  kind = "pipeline",
  detail,
  cascadeLabel,
}: Props) {
  // Default-OFF guarantee: the call site remounts this modal via a `key` keyed
  // on the delete target (which returns to a sentinel between opens), so the
  // instance is fresh on every open and `cascade` resets to `false` (#227).
  const [cascade, setCascade] = useState(false);

  if (!open) return null;

  return (
    <Modal
      size="s"
      title={`Delete this ${kind}?`}
      onClose={onClose}
      onSubmit={() => onConfirm(cascade)}
      // The dialog box: the *First pipeline* tour re-aims its last step onto
      // this rectangle so the projecteur's hole lands on Cancel/Delete (#825).
      data-testid="confirm-delete-modal"
      actions={
        <>
          <Button onClick={onClose}>Cancel</Button>
          <Button tone="danger" onClick={() => onConfirm(cascade)} data-autofocus="">
            Delete
          </Button>
        </>
      }
    >
      <div
        className="d-confirm"
        onKeyDown={(e) => {
          if (e.key === "Enter" && !e.ctrlKey && !e.metaKey) {
            e.preventDefault();
            onConfirm(cascade);
          }
        }}
      >
        <p className="d-confirm__line">{detail ?? "Removes its YAML and prompts"}</p>
        <code className="d-code d-confirm__target" title={name}>
          {name}
        </code>
        {cascadeLabel && (
          <label className="d-check">
            <input
              type="checkbox"
              data-testid="delete-cascade-checkbox"
              checked={cascade}
              onChange={(e) => setCascade(e.target.checked)}
            />
            {cascadeLabel}
          </label>
        )}
      </div>
    </Modal>
  );
}
