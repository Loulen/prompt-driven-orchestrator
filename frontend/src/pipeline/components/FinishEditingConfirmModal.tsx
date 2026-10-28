import { useEffect } from "react";

/**
 * ADR-0085: « Finish editing » with unsaved edits. Three ways out: keep
 * editing, save for this run then lock, or discard and lock. No Enter binding:
 * discarding is destructive and the user may have just been typing. Escape
 * keeps editing.
 */
export default function FinishEditingConfirmModal({
  onCancel,
  onDiscard,
  onSave,
}: {
  onCancel: () => void;
  onDiscard: () => void;
  onSave: () => void;
}) {
  useEffect(() => {
    function handleKey(e: KeyboardEvent) {
      if (e.key === "Escape") onCancel();
    }
    document.addEventListener("keydown", handleKey);
    return () => document.removeEventListener("keydown", handleKey);
  }, [onCancel]);

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-scrim"
      data-testid="finish-editing-backdrop"
      onClick={onCancel}
    >
      <div
        role="dialog"
        aria-label="Finish editing"
        data-testid="finish-editing-modal"
        className="flex w-[400px] max-w-[calc(90vw/var(--app-zoom,1))] flex-col rounded-lg border border-line bg-surface-1 p-4 shadow-lg"
        style={{ fontSize: "12px" }}
        onClick={(e) => e.stopPropagation()}
      >
        <h3 className="font-medium text-text" style={{ fontSize: "13px" }}>
          Finish editing with unsaved changes?
        </h3>
        <p className="mt-2 text-text-2">
          Your changes to this run are not saved. Finishing without saving throws them away; the run
          keeps its current pipeline.
        </p>
        <div className="mt-4 flex justify-end gap-2">
          <button
            onClick={onCancel}
            data-testid="finish-editing-cancel"
            className="cursor-pointer rounded-md border border-line-strong bg-surface-2 px-3 py-1.5 text-text-2 transition-colors hover:bg-hover"
            style={{ fontSize: "11.5px" }}
          >
            Keep editing
          </button>
          <button
            onClick={onDiscard}
            data-testid="finish-editing-discard"
            className="cursor-pointer rounded-md border border-st-blocked/50 bg-surface-2 px-3 py-1.5 text-st-blocked transition-colors hover:bg-hover"
            style={{ fontSize: "11.5px" }}
          >
            Discard changes
          </button>
          <button
            onClick={onSave}
            data-testid="finish-editing-save"
            className="cursor-pointer rounded-md bg-primary px-3 py-1.5 font-medium text-primary-ink transition-colors hover:bg-primary-hi"
            style={{ fontSize: "11.5px" }}
          >
            Save for this run
          </button>
        </div>
      </div>
    </div>
  );
}
