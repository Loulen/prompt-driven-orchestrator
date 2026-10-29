import { type Node, type NodeProps } from "@xyflow/react";
import { StickyNote } from "lucide-react";
import { useEditStore } from "../store/editStore";

/**
 * Data carried by a `note` canvas node (#307 / ADR-0018). A note is an inert
 * documentation post-it: no title, no port, no edge, never spawned, outside the
 * DAG and the runtime. Unlike a `loopRegion` box (decorative, non-interactive)
 * a note IS draggable and selectable — but never connectable (it renders no
 * `<Handle>`, so no edge can attach). The `note` xyflow node `type` is a canvas
 * concern only: it is NOT a PDO `NodeType`.
 */
export interface NoteNodeData {
  noteId: string;
  content: string;
  [key: string]: unknown;
}

/** A note card: a lightly tinted sheet, a small icon, the text. */
export function NoteNode({ data, id, selected }: NodeProps<Node<NoteNodeData>>) {
  const selection = useEditStore((s) => s.selection);
  // OR-in xyflow's own `selected` (mirror of `EditNode`, #232) so a note lit as
  // part of a multi-select group drag shows the ring too, not only the note the
  // Zustand single-selection tracks.
  const isSelected =
    selected || (selection.kind === "note" && selection.noteId === id);

  return (
    <div
      data-testid="note-card"
      data-note-id={data.noteId}
      data-selected={isSelected || undefined}
      className="cv-note"
    >
      <StickyNote size={12} strokeWidth={1.5} className="cv-note__ic" aria-label="Note" />
      {data.content ? (
        <p className="cv-note__text">{data.content}</p>
      ) : (
        <p className="cv-note__text" data-empty>
          Empty note
        </p>
      )}
    </div>
  );
}
