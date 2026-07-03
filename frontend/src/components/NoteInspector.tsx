import { StickyNote } from "lucide-react";
import { useEditStore } from "../stores/editStore";
import { SectionHead } from "./InspectorPrimitives";

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
    <aside
      className="flex h-full flex-col bg-bg-2 overflow-y-auto"
      data-testid="note-inspector"
    >
      <div className="flex items-center gap-2 border-b border-line px-3 py-2">
        <StickyNote size={14} className="shrink-0 text-acc" />
        <div className="min-w-0">
          <div className="truncate font-medium text-fg" style={{ fontSize: "12.5px" }}>
            Note
          </div>
          <div className="mt-0.5 text-fg-4" style={{ fontSize: "10px" }}>
            canvas note — documentation only
          </div>
        </div>
      </div>

      <div className="flex flex-col gap-3 p-3" style={{ fontSize: "11.5px" }}>
        <SectionHead title="Content" />
        <textarea
          value={note.content}
          onChange={(e) => updateNote(note.id, { content: e.target.value })}
          data-testid="note-content"
          rows={8}
          placeholder="Write a note…"
          className="w-full resize-y rounded border border-line-strong bg-bg-3 px-2 py-1.5 text-fg outline-none focus:border-acc"
          style={{ fontSize: "11.5px", lineHeight: 1.5 }}
        />
        <div className="text-fg-4" style={{ fontSize: "10px", lineHeight: 1.6 }}>
          A note is inert: it never runs, has no ports, and does not affect the
          pipeline diff (the synced/diverged star stays put).
        </div>
      </div>
    </aside>
  );
}
