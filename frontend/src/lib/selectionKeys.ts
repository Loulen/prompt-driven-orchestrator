import type { KeyboardEvent } from "react";
import type { SelectTab } from "../stores/selectionStore";

/**
 * Keyboard shortcuts for a left-panel list's multi-select (#577). Wired to the
 * list's own scroll container (not `window`) so it only fires when that list has
 * focus — Ctrl/Cmd-A must never hijack text-selection in the canvas or a field.
 *
 * - Ctrl/Cmd-A → select every currently-visible row.
 * - Escape     → clear the tab's selection (only when something is selected).
 * - Delete     → open the tab's destructive bulk confirm (only when selected).
 *
 * A no-op when focus is in a text input / textarea / contenteditable.
 */
interface SelectionKeyHandlers {
  tab: SelectTab;
  /** Ids in current visible order (post grouping/filtering). */
  visibleIds: string[];
  hasSelection: boolean;
  selectVisible: (tab: SelectTab, ids: string[]) => void;
  clear: (tab: SelectTab) => void;
  /** Opens the destructive bulk confirm for this tab. */
  onBulkDelete: () => void;
}

export function handleSelectionKeydown(
  e: KeyboardEvent,
  h: SelectionKeyHandlers,
): void {
  const target = e.target as HTMLElement | null;
  const tag = target?.tagName;
  if (tag === "INPUT" || tag === "TEXTAREA" || target?.isContentEditable) return;

  if ((e.metaKey || e.ctrlKey) && (e.key === "a" || e.key === "A")) {
    if (h.visibleIds.length === 0) return;
    e.preventDefault();
    h.selectVisible(h.tab, h.visibleIds);
    return;
  }
  if (e.key === "Escape" && h.hasSelection) {
    h.clear(h.tab);
    return;
  }
  if (e.key === "Delete" && h.hasSelection) {
    e.preventDefault();
    h.onBulkDelete();
  }
}
