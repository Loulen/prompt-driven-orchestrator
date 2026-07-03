import { type Node, type NodeProps } from "@xyflow/react";
import { StickyNote } from "lucide-react";
import { useEditStore } from "../stores/editStore";
import { SELECTION_RING_STYLE } from "../nodeStyles";

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

export function NoteNode({ data, id, selected }: NodeProps<Node<NoteNodeData>>) {
  const selection = useEditStore((s) => s.selection);
  // OR-in xyflow's own `selected` (mirror of `EditNode`, #232) so a note lit as
  // part of a multi-select group drag shows the accent ring too, not only the
  // note the Zustand single-selection tracks.
  const isSelected =
    selected || (selection.kind === "note" && selection.noteId === id);

  return (
    <div
      data-testid="note-card"
      data-note-id={data.noteId}
      className="rounded-md border border-dashed border-line-strong bg-st-await-bg px-3 py-2 text-fg-2"
      style={{
        width: 200,
        fontSize: "12px",
        whiteSpace: "pre-wrap",
        wordBreak: "break-word",
        ...(isSelected ? SELECTION_RING_STYLE : undefined),
      }}
    >
      <div className="mb-1 flex items-center gap-1.5 text-fg-4" style={{ fontSize: "10px" }}>
        <StickyNote size={12} className="shrink-0" />
        <span style={{ textTransform: "uppercase", letterSpacing: "0.05em" }}>Note</span>
      </div>
      {data.content ? (
        data.content
      ) : (
        <span className="text-fg-5 italic">Empty note — click to edit</span>
      )}
    </div>
  );
}
