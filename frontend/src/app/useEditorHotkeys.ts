import { useEffect } from "react";
import { handleUndoRedoKeydown } from "../lib/undoRedoHotkeys";
import { useEditStore } from "../pipeline/store/editStore";

/** Ctrl/Cmd+S saves the active tab; Ctrl/Cmd+Z / Shift+Z / Y undo and redo on
 *  the canvas. Both off without an edit tab and on a read-only archived run. */
export function useEditorHotkeys(hasEditTab: boolean, isActiveRunArchived: boolean) {
  const editSave = useEditStore((s) => s.save);
  const editUndo = useEditStore((s) => s.undo);
  const editRedo = useEditStore((s) => s.redo);
  const editActiveTabId = useEditStore((s) => s.activeTabId);
  useEffect(() => {
    // #315: never fire a save for an archived run — the tab is read-only and a
    // PUT would 404. `isActiveRunArchived` also removes this listener the moment
    // the open run flips to archived (via refreshRun).
    if (!hasEditTab || isActiveRunArchived) return;
    const handler = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === "s") {
        e.preventDefault();
        if (editActiveTabId) editSave(editActiveTabId);
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [hasEditTab, isActiveRunArchived, editActiveTabId, editSave]);

  // Canvas undo/redo (ADR-0014 / #226): Ctrl/Cmd+Z undo, Ctrl/Cmd+Shift+Z or
  // Ctrl/Cmd+Y redo. Sibling to the Ctrl+S effect above, but — unlike Save — it
  // MUST yield to native field undo while a text field is focused. The branch
  // logic (and that input-focus guard) lives in `handleUndoRedoKeydown` so it's
  // unit-testable without rendering the canvas; this effect just wires it up.
  useEffect(() => {
    // #315: undo/redo are edit affordances — off for a read-only archived run.
    if (!hasEditTab || isActiveRunArchived) return;
    const handler = (e: KeyboardEvent) => handleUndoRedoKeydown(e, editUndo, editRedo);
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [hasEditTab, isActiveRunArchived, editUndo, editRedo]);
}
