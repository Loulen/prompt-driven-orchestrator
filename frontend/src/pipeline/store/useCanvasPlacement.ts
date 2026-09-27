/**
 * Where a new node or note lands: in view, clear of every card already on the canvas (#825).
 */

import type { ReactFlowInstance } from "@xyflow/react";
import type { EditorPipelineDef } from "../model/pipeline";
import type { NoteDef } from "../../shared/generated/NoteDef";
import type { Selection } from "./editState";
import type { NodeType } from "../../shared/generated/NodeType";
import { generateNodeId } from "../../lib/nanoid";
import { CARD_HEIGHT, CARD_WIDTH, fallbackNodeSpot, freeDropSpot } from "../../lib/nodePlacement";
// #723 — orchestrator pastilles on the run-view node card.
import type { EditorNodeDef } from "../model/pipeline";
const DEFAULT_NODE_NAMES: Partial<Record<NodeType, string>> = {
  "agent": "implementer",
  "merge": "merge",
  "script": "script",
};

export function useCanvasPlacement({
  pipeline,
  reactFlowRef,
  reactFlow,
  addNodeToStore,
  addNoteToStore,
  setSelection,
}: {
  pipeline: EditorPipelineDef | undefined;
  reactFlowRef: React.RefObject<HTMLDivElement | null>;
  reactFlow: ReactFlowInstance;
  addNodeToStore: (node: EditorNodeDef) => void;
  addNoteToStore: (note: NoteDef) => void;
  setSelection: (sel: Selection) => void;
}) {
  /**
   * What the reader can actually see, in canvas units (#825, FP iteration 2). A
   * drop search that knows only canvas units puts the second card of a fresh
   * pipeline off the right edge of the screen, because « one card to the right »
   * is 440 pixels at the zoom a two-marker canvas fit-views to.
   *
   * `null` when there is nothing to measure — jsdom, or a canvas not laid out
   * yet — and the search then walks blind rather than inside a zero-sized box.
   */
  const visibleCanvasRect = (): { x: number; y: number; width: number; height: number } | null => {
    const wrapper = reactFlowRef.current;
    if (!wrapper) return null;
    const rect = wrapper.getBoundingClientRect();
    if (rect.width <= 0 || rect.height <= 0) return null;
    const topLeft = reactFlow.screenToFlowPosition({ x: rect.left, y: rect.top });
    const bottomRight = reactFlow.screenToFlowPosition({ x: rect.right, y: rect.bottom });
    const width = bottomRight.x - topLeft.x;
    const height = bottomRight.y - topLeft.y;
    if (!(width > 0) || !(height > 0)) return null;
    return { x: topLeft.x, y: topLeft.y, width, height };
  };

  const computeDropPosition = (): { x: number; y: number } => {
    const visible = visibleCanvasRect();
    let cx: number;
    let cy: number;
    if (visible) {
      cx = visible.x + visible.width / 2;
      cy = visible.y + visible.height / 2;
    } else {
      cx = 200;
      cy = 200;
    }
    // Moved aside until it clears every card already on the canvas — footprints,
    // not a few pixels (`lib/nodePlacement.ts`): a card dropped 40 pixels off
    // another one is a card on top of another one, and the edges between them
    // are then unreachable.
    // A node with no persisted position is on screen too — a fresh pipeline's
    // Start and End are written without a `view`, and the search that could not
    // see them dropped the first agent right on End.
    const occupied = (pipeline?.nodes ?? []).map((n, i) => {
      const fallback = fallbackNodeSpot(i);
      return { x: n.view?.x ?? fallback.x, y: n.view?.y ?? fallback.y };
    });
    return freeDropSpot(
      { x: Math.round(cx - CARD_WIDTH / 2), y: Math.round(cy - CARD_HEIGHT / 2) },
      occupied,
      visible,
    );
  };

  const handleAddNode = (type: NodeType) => {
    const id = generateNodeId();
    const name = DEFAULT_NODE_NAMES[type] ?? "node";

    const view = computeDropPosition();
    let newNode: EditorNodeDef;
    switch (type) {
      case "merge":
        newNode = {
          id, name, type, interactive: false, view,
          inputs: [{ name: "branches", repeated: true, side: "left" }],
          outputs: [{ name: "merged", repeated: false, side: "right" }],
        };
        break;
      case "script":
        // #248: a script node's inputs are emergent (edge-derived), like a work
        // node — it declares none. One default output for its `output.md`.
        // #653: a new Script shares the Run worktree — the inverse default of an
        // Agent, so lightweight runtime and artifact work stays lightweight.
        newNode = {
          id, name, type, interactive: false, view,
          isolated_worktree: false,
          inputs: [],
          outputs: [{ name: "out", repeated: false, side: "right" }],
        };
        break;
      default:
        newNode = {
          id, name, type, interactive: false, view,
          // #653/ADR-0060: a new Agent is isolated. The safe placement is the
          // default, and sharing the Run worktree is an explicit opt-out.
          isolated_worktree: true,
          inputs: [{ name: "in", repeated: false, side: "left" }],
          outputs: [{ name: "out", repeated: false, side: "right" }],
        };
    }
    addNodeToStore(newNode);
  };

  const handleAddNote = () => {
    // A note (#307 / ADR-0018) is created empty and selected, positioned at the
    // viewport centre — same drop logic as a node. It carries no ports/name/type.
    const id = generateNodeId();
    const view = computeDropPosition();
    addNoteToStore({ id, content: "", view });
    setSelection({ kind: "note", id: null, noteId: id });
  };
  return { computeDropPosition, handleAddNode, handleAddNote };
}
