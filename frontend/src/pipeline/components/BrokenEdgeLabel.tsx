import type { MouseEvent } from "react";
import { Trash2, Unlink } from "lucide-react";
import type { Point } from "../../lib/orthogonalRouter";

interface BrokenEdgeLabelProps {
  edgeId: string;
  /** The carried ports the source no longer declares. */
  ports: string[];
  /** Flow-space centre of the label. */
  at: Point;
  zIndex: number;
  /** Deletes the edge (Undo restores it). Absent on a locked canvas: no bin. */
  onDelete?: () => void;
  onContextMenu: (ev: MouseEvent) => void;
}

/**
 * CORE-88: a broken edge says which port is gone, on the wire, with a direct
 * delete. Rendered inside the edge's `EdgeLabelRenderer`.
 */
export default function BrokenEdgeLabel({ edgeId, ports, at, zIndex, onDelete, onContextMenu }: BrokenEdgeLabelProps) {
  return (
    <div
      className="nodrag nopan"
      data-testid={`edge-broken-label-${edgeId}`}
      onContextMenu={onContextMenu}
      style={{
        position: "absolute",
        transform: `translate(-50%, -50%) translate(${at.x}px, ${at.y}px)`,
        display: "flex",
        alignItems: "center",
        gap: 6,
        fontFamily: "var(--font-code, monospace)",
        fontSize: 10,
        color: "var(--color-st-failed)",
        // The red tint is translucent: laid over an opaque fill, or the wire
        // under the label would cut through its text.
        background:
          "linear-gradient(color-mix(in oklab, var(--color-st-failed) 14%, transparent), color-mix(in oklab, var(--color-st-failed) 14%, transparent)), var(--surface-1)",
        border: "1px solid var(--color-st-failed)",
        borderRadius: 4,
        padding: "2px 6px",
        pointerEvents: "all",
        zIndex,
        whiteSpace: "nowrap",
      }}
    >
      <Unlink size={11} aria-hidden="true" />
      <span>{ports.join(", ")} · port absent</span>
      {onDelete && (
        <button
          type="button"
          aria-label="Delete broken edge"
          title="Delete this edge"
          data-testid={`edge-broken-delete-${edgeId}`}
          onPointerDown={(ev) => ev.stopPropagation()}
          onClick={(ev) => {
            ev.stopPropagation();
            onDelete();
          }}
          style={{ display: "flex", cursor: "pointer", color: "inherit" }}
        >
          <Trash2 size={11} />
        </button>
      )}
    </div>
  );
}
