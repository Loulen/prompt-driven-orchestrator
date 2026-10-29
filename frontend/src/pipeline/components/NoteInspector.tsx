import { StickyNote } from "lucide-react";
import { useEditStore } from "../store/editStore";
import "./inspect.css";

/**
 * Inspector for an inert canvas note (#307 / ADR-0018). Opened by clicking a
 * note on the canvas. A note has no title, no ports, no type, no model — just
 * free-text `content`, edited here in a single textarea. Editing is COW and
 * undo-tracked (`updateNote`); it flips the tab's dirty flag but never moves the
 * synced/diverged star (a note is layout, not semantics — see
 * `comparablePipelineObject`). Content is plain text in v1 (no markdown render).
 */
export default function NoteInspector() {
  const openTabs = useEditStore((s) => s.openTabs);
  const activeTabId = useEditStore((s) => s.activeTabId);
  const selection = useEditStore((s) => s.selection);
  const updateNote = useEditStore((s) => s.updateNote);

  const tab = openTabs.find((t) => t.id === activeTabId);
  if (!tab || selection.kind !== "note" || !selection.noteId) return null;

  const note = (tab.pipeline.notes ?? []).find((n) => n.id === selection.noteId);
  if (!note) return null;

  return (
    <aside className="ei" data-testid="note-inspector">
      <div className="ei-bar">
        <span className="ei-bar__title" title="Canvas note: never runs, no port, not in the diff">
          <StickyNote size={14} strokeWidth={1.5} aria-hidden />
          <span>Note</span>
          <span className="ei-bar__sub">{note.id}</span>
        </span>
      </div>
      <textarea
        value={note.content}
        onChange={(e) => updateNote(note.id, { content: e.target.value })}
        data-testid="note-content"
        aria-label="Note"
        rows={8}
        placeholder="Note"
        className="d-input d-area ei-note-area"
      />
    </aside>
  );
}
