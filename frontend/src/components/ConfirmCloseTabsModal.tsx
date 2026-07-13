import { useEffect } from "react";
import type { OpenPipeline } from "../stores/editStore";
import { hasUnsavedWork } from "../stores/editStore";

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
 * unsaved work (#342). Replicated locally rather than sharing `DestroyLoopModal`
 * (same house pattern as #339): the two modals only look alike, coupling them
 * would drag loop-specific copy into the tabs feature.
 *
 * Deliberately no Enter→confirm binding (unlike `DestroyLoopModal`): this is a
 * destructive action and the user may have just been typing — an accidental
 * Enter must not discard their work. Escape cancels.
 */
export default function ConfirmCloseTabsModal({ open, tabs, onCancel, onConfirm }: Props) {
  useEffect(() => {
    if (!open) return;
    function handleKey(e: KeyboardEvent) {
      if (e.key === "Escape") onCancel();
    }
    document.addEventListener("keydown", handleKey);
    return () => document.removeEventListener("keydown", handleKey);
  }, [open, onCancel]);

  if (!open) return null;

  const unsaved = tabs.filter(hasUnsavedWork);
  const plural = unsaved.length > 1;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60"
      data-testid="close-tabs-backdrop"
      onClick={onCancel}
    >
      <div
        className="w-[380px] rounded-lg border border-line bg-bg-2 p-4 shadow-lg"
        style={{ fontSize: "12px" }}
        onClick={(e) => e.stopPropagation()}
      >
        <h3 className="font-medium text-fg" style={{ fontSize: "13px" }}>
          Discard unsaved changes?
        </h3>
        <p className="mt-2 text-fg-2">
          {plural ? `${unsaved.length} tabs have` : "1 tab has"} unsaved changes:{" "}
          {unsaved.map((tab, i) => (
            <span key={tab.id}>
              {i > 0 && ", "}
              <code className="rounded bg-bg-4 px-1 py-0.5 font-mono text-fg">
                {tab.id}.yaml
              </code>
            </span>
          ))}
          . Closing {plural ? "them" : "it"} throws those changes away.
        </p>
        <div className="mt-4 flex justify-end gap-2">
          <button
            onClick={onCancel}
            data-testid="close-tabs-cancel"
            className="rounded-md border border-line-strong bg-bg-3 px-3 py-1.5 text-fg-2 transition-colors hover:bg-bg-4"
            style={{ fontSize: "11.5px" }}
          >
            Cancel
          </button>
          <button
            onClick={onConfirm}
            data-testid="close-tabs-confirm"
            className="rounded-md bg-st-failed px-3 py-1.5 text-white transition-colors hover:bg-st-failed/80"
            style={{ fontSize: "11.5px" }}
          >
            Close {plural ? "tabs" : "tab"} anyway
          </button>
        </div>
      </div>
    </div>
  );
}
