/**
 * The edit canvas's right-click menu.
 */

import DestroyLoopModal from "../../components/DestroyLoopModal";
import { regionsDestroyedByEdgeRemoval } from "../../lib/loopRegions";
import type { EditorPipelineDef } from "../model/pipeline";
import { useEditStore } from "../store/editStore";
import { useEffect, useState } from "react";
// #723 — orchestrator pastilles on the run-view node card.
/** The context menu the canvas holds open: the element and where. */
export interface CanvasMenu {
  x: number;
  y: number;
  type: "node" | "edge" | "note";
  id: string;
  edgeIndex?: number;
  fanoutMembers?: string[];
  fanoutFields?: string[];
}

export function ContextMenu({
  x,
  y,
  type,
  fanoutFields,
  onDeleteNode,
  onDeleteNote,
  onDuplicateNode,
  onExportNode,
  onFanOut,
  onDeleteEdge,
  onClose,
}: {
  x: number;
  y: number;
  type: "node" | "edge" | "note";
  fanoutFields?: string[];
  onDeleteNode: () => void;
  onDeleteNote: () => void;
  onDuplicateNode: () => void;
  onExportNode: () => void;
  onFanOut: (field: string) => void;
  onDeleteEdge: () => void;
  onClose: () => void;
}) {
  // Escape dismisses the menu. Without it the full-screen backdrop below stayed
  // mounted after the menu was abandoned by keyboard, and silently swallowed the
  // next click on the canvas or the edge panel (#844 FP iter-2).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <>
      <div
        className="fixed inset-0 z-40"
        data-testid="context-menu-backdrop"
        onClick={onClose}
        onContextMenu={(e) => {
          e.preventDefault();
          onClose();
        }}
      />
      <div
        className="fixed z-50 rounded-md border border-line bg-bg-3 py-1 shadow-lg"
        style={{ left: x, top: y, fontSize: "11.5px", minWidth: 140 }}
      >
        {type === "node" ? (
          <>
            <button
              onClick={onDuplicateNode}
              className="flex w-full cursor-pointer items-center px-3 py-1.5 text-left text-fg-2 hover:bg-bg-4 hover:text-fg"
            >
              Duplicate
            </button>
            {/* "Fan out over a collection" (#151 / #269): one entry per
                eligible `list` field flowing into the selection. Absent when
                nothing is eligible — the gesture stays discoverable via the
                lint-banner nudge. */}
            {(fanoutFields ?? []).map((field) => (
              <button
                key={field}
                data-testid={`ctx-fanout-${field}`}
                onClick={() => onFanOut(field)}
                className="flex w-full cursor-pointer items-center px-3 py-1.5 text-left text-fg-2 hover:bg-bg-4 hover:text-fg"
              >
                Fan out over "{field}"
              </button>
            ))}
            {/* #345: front-only export of this node to YAML (no daemon). */}
            <button
              data-testid="ctx-export-node"
              onClick={onExportNode}
              className="flex w-full cursor-pointer items-center px-3 py-1.5 text-left text-fg-2 hover:bg-bg-4 hover:text-fg"
            >
              Export as YAML…
            </button>
            <button
              onClick={onDeleteNode}
              className="flex w-full cursor-pointer items-center px-3 py-1.5 text-left text-st-failed hover:bg-bg-4"
            >
              Delete
            </button>
          </>
        ) : type === "note" ? (
          // #307 trap #2: a note's Delete must call `deleteNote`, not
          // `deleteNode` (which would no-op on an id absent from pipeline.nodes).
          <button
            data-testid="context-menu-delete"
            onClick={onDeleteNote}
            className="flex w-full cursor-pointer items-center px-3 py-1.5 text-left text-st-failed hover:bg-bg-4"
          >
            Delete note
          </button>
        ) : (
          <button
            onClick={onDeleteEdge}
            className="flex w-full cursor-pointer items-center px-3 py-1.5 text-left text-st-failed hover:bg-bg-4"
          >
            Delete edge
          </button>
        )}
      </div>
    </>
  );
}

/** The open context menu of the canvas, what each of its entries does, and
 * the destroy-loop confirmation a Delete-edge may need (#150). */
export function CanvasContextMenuHost({
  contextMenu,
  setContextMenu,
  pipeline,
  onExportNode,
}: {
  contextMenu: CanvasMenu | null;
  setContextMenu: (menu: CanvasMenu | null) => void;
  pipeline: EditorPipelineDef;
  onExportNode: (nodeId: string) => void;
}) {
  const deleteNode = useEditStore((s) => s.deleteNode);
  const duplicateNode = useEditStore((s) => s.duplicateNode);
  const createCollectionRegion = useEditStore((s) => s.createCollectionRegion);
  const deleteEdge = useEditStore((s) => s.deleteEdge);
  const deleteNote = useEditStore((s) => s.deleteNote);
  // Pending destroy-loop confirmation (#150): set when a Delete-edge action would
  // remove a bounded region's last cycle. Holds the edge to delete and the loops
  // it would destroy; confirming deletes the edge (the store drops the regions).
  const [pendingDestroy, setPendingDestroy] = useState<{
    edgeIndex: number;
    loopIds: string[];
  } | null>(null);
  const setExportNodeId = onExportNode;
  return (
    <>
      {contextMenu && (
        <ContextMenu
          {...contextMenu}
          onDeleteNode={() => {
            deleteNode(contextMenu.id);
            setContextMenu(null);
          }}
          onDeleteNote={() => {
            deleteNote(contextMenu.id);
            setContextMenu(null);
          }}
          onDuplicateNode={() => {
            duplicateNode(contextMenu.id);
            setContextMenu(null);
          }}
          onExportNode={() => {
            setExportNodeId(contextMenu.id);
            setContextMenu(null);
          }}
          onFanOut={(field) => {
            if (contextMenu.fanoutMembers && contextMenu.fanoutMembers.length > 0) {
              createCollectionRegion(contextMenu.fanoutMembers, field);
            }
            setContextMenu(null);
          }}
          onDeleteEdge={() => {
            const idx = contextMenu.edgeIndex;
            setContextMenu(null);
            if (idx === undefined) return;
            // Destroy-loop confirmation (ADR-0011 / #150): if this edge is the
            // last cycle of one or more bounded regions, confirm before deleting
            // (the store removes the destroyed `loops:` entries on confirm).
            // Deleting a non-last cycle edge proceeds immediately (no popup).
            const destroyed = pipeline
              ? regionsDestroyedByEdgeRemoval(pipeline, idx)
              : [];
            if (destroyed.length > 0) {
              setPendingDestroy({ edgeIndex: idx, loopIds: destroyed });
            } else {
              deleteEdge(idx);
            }
          }}
          onClose={() => setContextMenu(null)}
        />
      )}

      <DestroyLoopModal
        open={pendingDestroy != null}
        loopIds={pendingDestroy?.loopIds ?? []}
        onClose={() => setPendingDestroy(null)}
        onConfirm={() => {
          if (pendingDestroy) deleteEdge(pendingDestroy.edgeIndex);
          setPendingDestroy(null);
        }}
      />
    </>
  );
}
