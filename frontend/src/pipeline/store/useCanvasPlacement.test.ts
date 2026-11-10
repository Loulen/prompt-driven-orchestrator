import { describe, expect, it, vi } from "vitest";
import { renderHook } from "@testing-library/react";
import type { ReactFlowInstance } from "@xyflow/react";
import { useCanvasPlacement } from "./useCanvasPlacement";
import type { EditorPipelineDef } from "../model/pipeline";
import type { NoteDef } from "../../shared/generated/NoteDef";
import { CARD_HEIGHT, CARD_WIDTH } from "../../lib/nodePlacement";

function pipelineWithNotes(notes: NoteDef[]): EditorPipelineDef {
  return { name: "flow", version: "1.0", variables: {}, nodes: [], edges: [], notes } as EditorPipelineDef;
}

/** No canvas laid out (jsdom): the drop point is (200, 200), the card centred on it. */
const BLIND_DROP = { x: 200 - CARD_WIDTH / 2, y: 200 - CARD_HEIGHT / 2 };

function place(pipeline: EditorPipelineDef) {
  const addNoteToStore = vi.fn();
  const { result } = renderHook(() =>
    useCanvasPlacement({
      pipeline,
      reactFlowRef: { current: null },
      reactFlow: {} as ReactFlowInstance,
      addNodeToStore: vi.fn(),
      addNoteToStore,
      setSelection: vi.fn(),
    }),
  );
  return { result, addNoteToStore };
}

describe("useCanvasPlacement — a new note", () => {
  it("lands clear of a note already at the drop point", () => {
    const { result, addNoteToStore } = place(
      pipelineWithNotes([{ id: "n1", content: "first", view: { ...BLIND_DROP } }]),
    );

    result.current.handleAddNote();

    const added = addNoteToStore.mock.calls[0][0] as NoteDef;
    expect(added.view).not.toEqual(BLIND_DROP);
    expect(Math.abs(added.view!.x - BLIND_DROP.x) >= CARD_WIDTH || Math.abs(added.view!.y - BLIND_DROP.y) >= CARD_HEIGHT).toBe(true);
  });

  it("counts a note with no saved position where the canvas draws it", () => {
    // The canvas draws an unplaced note at (240, 80 + index × 120): the second one
    // at (240, 200), across the drop point.
    const { result } = place(
      pipelineWithNotes([
        { id: "n1", content: "first" },
        { id: "n2", content: "second" },
      ]),
    );

    const spot = result.current.computeDropPosition();

    expect(Math.abs(spot.x - 240) >= CARD_WIDTH || Math.abs(spot.y - 200) >= CARD_HEIGHT).toBe(true);
  });
});
