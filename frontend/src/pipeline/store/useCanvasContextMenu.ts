/**
 * The canvas's right-click menu: which element, and for a node the collection fan-out it offers (#151 / #269).
 */

import type { EditorPipelineDef } from "../model/pipeline";
import { useCallback, useState } from "react";
import { type Node, type Edge } from "@xyflow/react";
import { collectionFanoutFields } from "../../lib/loopRegions";
// #723 — orchestrator pastilles on the run-view node card.

export function useCanvasContextMenu(pipeline: EditorPipelineDef | undefined, nodes: Node[]) {
  const [contextMenu, setContextMenu] = useState<{
    x: number;
    y: number;
    type: "node" | "edge" | "note";
    id: string;
    edgeIndex?: number;
    // "Fan out over a collection" gesture (#151 / #269): the member set the
    // gesture would wrap (clicked node + xyflow multi-selection, start/end
    // excluded) and the eligible `list` field names, one menu entry each.
    // Empty/absent when the selection isn't eligible.
    fanoutMembers?: string[];
    fanoutFields?: string[];
  } | null>(null);

  const handleNodeContextMenu = useCallback(
    (event: React.MouseEvent, node: Node) => {
      event.preventDefault();
      // Loop-region boxes are decorative, not pipeline nodes — no context menu.
      if (node.type === "loopRegion") return;
      // A note (#307 trap #2) is NOT in `pipeline.nodes`, so the node lookup
      // below would miss it and its Delete would call `deleteNode(noteId)` — a
      // no-op. Give it its own `"note"` menu whose Delete routes to `deleteNote`.
      if (node.type === "note") {
        setContextMenu({
          x: event.clientX,
          y: event.clientY,
          type: "note",
          id: node.id,
        });
        return;
      }
      const nodeDef = pipeline?.nodes.find((n) => n.id === node.id);
      if (nodeDef?.type === "start" || nodeDef?.type === "end") return;
      // "Fan out over a collection" (#151 / #269): members = the clicked node
      // plus any other xyflow-selected pipeline nodes (a right-click on one
      // node of a multi-selection fans out the whole group). Start/end are
      // structural markers, never members. The gesture is offered only when
      // some incoming edge from a non-member carries a `list` field AND no
      // member already lives in a region (a node is in at most one).
      let fanoutMembers: string[] = [];
      let fanoutFields: string[] = [];
      if (pipeline) {
        const memberIds = new Set([node.id]);
        for (const n of nodes) {
          if (n.selected) memberIds.add(n.id);
        }
        fanoutMembers = pipeline.nodes
          .filter((n) => memberIds.has(n.id) && n.type !== "start" && n.type !== "end")
          .map((n) => n.id);
        const inRegion = new Set(
          (pipeline.loops ?? []).flatMap((r) => r.members),
        );
        fanoutFields = fanoutMembers.some((m) => inRegion.has(m))
          ? []
          : collectionFanoutFields(pipeline, fanoutMembers);
      }
      setContextMenu({
        x: event.clientX,
        y: event.clientY,
        type: "node",
        id: node.id,
        fanoutMembers,
        fanoutFields,
      });
    },
    [pipeline, nodes],
  );

  const handleEdgeContextMenu = useCallback(
    (event: React.MouseEvent, edge: Edge) => {
      event.preventDefault();
      const idx = parseInt(edge.id.replace("e-", ""), 10);
      setContextMenu({
        x: event.clientX,
        y: event.clientY,
        type: "edge",
        id: edge.id,
        edgeIndex: idx,
      });
    },
    [],
  );
  return { contextMenu, setContextMenu, handleNodeContextMenu, handleEdgeContextMenu };
}
