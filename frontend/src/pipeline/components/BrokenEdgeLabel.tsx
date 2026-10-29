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
      className="nodrag nopan cv-broken"
      data-testid={`edge-broken-label-${edgeId}`}
      onContextMenu={onContextMenu}
      title={`${ports.join(", ")} · port absent`}
      style={{
        position: "absolute",
        transform: `translate(-50%, -50%) translate(${at.x}px, ${at.y}px)`,
        pointerEvents: "all",
        zIndex,
      }}
    >
      <Unlink size={11} strokeWidth={1.5} aria-hidden="true" />
      <span>{ports.join(", ")} · port absent</span>
      {onDelete && (
        <button
          type="button"
          aria-label="Delete broken edge"
          title="Delete edge"
          data-testid={`edge-broken-delete-${edgeId}`}
          onPointerDown={(ev) => ev.stopPropagation()}
          onClick={(ev) => {
            ev.stopPropagation();
            onDelete();
          }}
        >
          <Trash2 size={11} strokeWidth={1.5} aria-hidden />
        </button>
      )}
    </div>
  );
}
