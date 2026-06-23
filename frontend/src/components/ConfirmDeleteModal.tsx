import { useEffect, useState } from "react";

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
  detail?: string;
  /**
   * When set, render an opt-in checkbox (default unchecked) with this label
   * between the detail text and the buttons. Its state flows through
   * `onConfirm(cascade)`. Absent ⇒ no checkbox, `onConfirm(false)` (#227).
   */
  cascadeLabel?: string;
}

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

  useEffect(() => {
    if (!open) return;
    function handleKey(e: KeyboardEvent) {
      if (e.key === "Escape") onClose();
      // Capture the latest checkbox state — `cascade` is in the dep array so
      // the Enter path doesn't confirm with a stale `false`.
      if (e.key === "Enter") onConfirm(cascade);
    }
    document.addEventListener("keydown", handleKey);
    return () => document.removeEventListener("keydown", handleKey);
  }, [open, onClose, onConfirm, cascade]);

  if (!open) return null;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60"
      data-testid="confirm-delete-backdrop"
      onClick={onClose}
    >
      <div
        className="w-[360px] rounded-lg border border-line bg-bg-2 p-4 shadow-lg"
        style={{ fontSize: "12px" }}
        onClick={(e) => e.stopPropagation()}
      >
        <h3 className="font-medium text-fg" style={{ fontSize: "13px" }}>
          Delete this {kind}?
        </h3>
        <p className="mt-2 text-fg-2">
          <code className="rounded bg-bg-4 px-1 py-0.5 font-mono text-fg">
            {name}
          </code>
        </p>
        <p className="mt-2 text-fg-3" style={{ fontSize: "11.5px" }}>
          {detail ??
            "This will permanently remove the YAML file and its prompt files from disk. This action cannot be undone."}
        </p>
        {cascadeLabel && (
          <label
            className="mt-3 flex cursor-pointer items-center gap-2 text-fg-2"
            style={{ fontSize: "11.5px" }}
            onClick={(e) => e.stopPropagation()}
          >
            <input
              type="checkbox"
              data-testid="delete-cascade-checkbox"
              checked={cascade}
              onChange={(e) => setCascade(e.target.checked)}
            />
            {cascadeLabel}
          </label>
        )}
        <div className="mt-4 flex justify-end gap-2">
          <button
            onClick={onClose}
            className="rounded-md border border-line-strong bg-bg-3 px-3 py-1.5 text-fg-2 transition-colors hover:bg-bg-4"
            style={{ fontSize: "11.5px" }}
          >
            Cancel
          </button>
          <button
            onClick={() => onConfirm(cascade)}
            className="rounded-md bg-st-failed px-3 py-1.5 text-white transition-colors hover:bg-st-failed/80"
            style={{ fontSize: "11.5px" }}
          >
            Delete
          </button>
        </div>
      </div>
    </div>
  );
}
