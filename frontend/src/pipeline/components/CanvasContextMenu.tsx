/**
 * The edit canvas's right-click menu, on the shared `DropdownMenu` (#30),
 * anchored where the pointer was. After the core's entries of a node or an
 * edge, the extensions' `canvas/node/menu` or `canvas/edge/menu` groups.
 */

import { Copy, FileDown, Split, Trash2 } from "lucide-react";
import DestroyLoopModal from "./DestroyLoopModal";
import { regionsDestroyedByEdgeRemoval } from "../../lib/loopRegions";
import type { EditorPipelineDef } from "../model/pipeline";
import { useEditStore } from "../store/editStore";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { RunState } from "../../shared/generated/RunState";
import { ExtensionMenuGroups } from "../../shared/extensions/ExtensionActions";
import { canvasMenuExtensions, type CanvasMenuExtensions } from "../model/canvasMenuExtensions";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
} from "../../shared/ui/dropdown-menu";
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

const ITEM = "cv-menu__item";

export function ContextMenu({
  x,
  y,
  type,
  fanoutFields,
  extensions,
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
  extensions?: CanvasMenuExtensions | null;
  onDeleteNode: () => void;
  onDeleteNote: () => void;
  onDuplicateNode: () => void;
  onExportNode: () => void;
  onFanOut: (field: string) => void;
  onDeleteEdge: () => void;
  onClose: () => void;
}) {
  // The menu closes once, whichever way: the menu's own dismissal (Escape
  // inside it, a click away), the backdrop, or Escape anywhere.
  const closed = useRef(false);
  const closeOnce = useCallback(() => {
    if (closed.current) return;
    closed.current = true;
    onClose();
  }, [onClose]);

  // Escape dismisses the menu. Without it the full-screen backdrop below stayed
  // mounted after the menu was abandoned by keyboard, and silently swallowed the
  // next click on the canvas or the edge panel (#844 FP iter-2).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") closeOnce();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [closeOnce]);

  // The pointer's position, as the element the menu is anchored to.
  const anchor = useMemo(
    () => ({ getBoundingClientRect: () => DOMRect.fromRect({ x, y, width: 0, height: 0 }) }),
    [x, y],
  );

  return (
    <>
      <div
        className="fixed inset-0 z-40"
        data-testid="context-menu-backdrop"
        onClick={closeOnce}
        onContextMenu={(e) => {
          e.preventDefault();
          closeOnce();
        }}
      />
      <DropdownMenu
        open
        // The backdrop above holds the outside clicks, as it always did.
        modal={false}
        onOpenChange={(open) => {
          if (!open) closeOnce();
        }}
      >
        <DropdownMenuContent
          anchor={anchor}
          side="bottom"
          align="start"
          sideOffset={0}
          className="cv-menu w-auto"
          data-testid="canvas-context-menu"
        >
          {type === "node" ? (
            <>
              <DropdownMenuItem className={ITEM} onClick={onDuplicateNode}>
                <Copy size={14} strokeWidth={1.5} aria-hidden />
                Duplicate
              </DropdownMenuItem>
              {/* "Fan out over a collection" (#151 / #269): one entry per
                  eligible `list` field flowing into the selection. Absent when
                  nothing is eligible — the gesture stays discoverable via the
                  lint-banner nudge. */}
              {(fanoutFields ?? []).map((field) => (
                <DropdownMenuItem
                  key={field}
                  data-testid={`ctx-fanout-${field}`}
                  className={ITEM}
                  onClick={() => onFanOut(field)}
                >
                  <Split size={14} strokeWidth={1.5} aria-hidden />
                  Fan out over "{field}"
                </DropdownMenuItem>
              ))}
              {/* #345: front-only export of this node to YAML (no daemon). */}
              <DropdownMenuItem data-testid="ctx-export-node" className={ITEM} onClick={onExportNode}>
                <FileDown size={14} strokeWidth={1.5} aria-hidden />
                Export as YAML…
              </DropdownMenuItem>
              {extensions?.location === "canvas/node/menu" && (
                <ExtensionMenuGroups location="canvas/node/menu" extra={extensions.extra} itemClassName={ITEM} trailing />
              )}
              <DropdownMenuItem variant="destructive" data-testid="ctx-delete-node" className={ITEM} onClick={onDeleteNode}>
                <Trash2 size={14} strokeWidth={1.5} aria-hidden />
                Delete
              </DropdownMenuItem>
            </>
          ) : type === "note" ? (
            // #307 trap #2: a note's Delete must call `deleteNote`, not
            // `deleteNode` (which would no-op on an id absent from pipeline.nodes).
            <DropdownMenuItem
              variant="destructive"
              data-testid="context-menu-delete"
              className={ITEM}
              onClick={onDeleteNote}
            >
              <Trash2 size={14} strokeWidth={1.5} aria-hidden />
              Delete note
            </DropdownMenuItem>
          ) : (
            <>
              {extensions?.location === "canvas/edge/menu" && (
                <ExtensionMenuGroups
                  location="canvas/edge/menu"
                  extra={extensions.extra}
                  itemClassName={ITEM}
                  leading={false}
                  trailing
                />
              )}
              <DropdownMenuItem variant="destructive" data-testid="ctx-delete-edge" className={ITEM} onClick={onDeleteEdge}>
                <Trash2 size={14} strokeWidth={1.5} aria-hidden />
                Delete edge
              </DropdownMenuItem>
            </>
          )}
        </DropdownMenuContent>
      </DropdownMenu>
    </>
  );
}

/** The open context menu of the canvas, what each of its entries does, and
 * the destroy-loop confirmation a Delete-edge may need (#150). */
export function CanvasContextMenuHost({
  contextMenu,
  setContextMenu,
  pipeline,
  run = null,
  onExportNode,
}: {
  contextMenu: CanvasMenu | null;
  setContextMenu: (menu: CanvasMenu | null) => void;
  pipeline: EditorPipelineDef;
  /** The Run of a Run's canvas. */
  run?: RunState | null;
  onExportNode: (nodeId: string) => void;
}) {
  // The open tab: the pipeline the extensions' entries receive.
  const pipelineId = useEditStore((s) => s.activeTabId) ?? pipeline.name;
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
          // A new right-click is a new menu.
          key={`${contextMenu.type}:${contextMenu.id}:${contextMenu.x}:${contextMenu.y}`}
          {...contextMenu}
          extensions={canvasMenuExtensions(contextMenu, pipeline, pipelineId, run)}
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
