import { beforeEach, describe, expect, it } from "vitest";
import { renderHook } from "@testing-library/react";
import type { Node } from "@xyflow/react";
import type { EditorNodeDef, EditorPipelineDef } from "../model/pipeline";
import { grownCanvas } from "../model/grownCanvas";
import { IDENTITY_SHIFT, storedEdgePatch } from "./canvasShift";
import { useEditStore } from "./editStore";
import { useCanvasNodeDragStop } from "./useCanvasNodeDragStop";

const node = (id: string, y: number, outputs = 1): EditorNodeDef => ({
  id,
  name: id,
  type: "agent",
  inputs: [],
  outputs: Array.from({ length: outputs }, (_, i) => ({ name: `out${i}` })),
  interactive: false,
  view: { x: 0, y },
});

/** Design (three outputs, +32 px) on the first row, Review under it. */
function pipeline(): EditorPipelineDef {
  return {
    name: "p",
    version: "1.0",
    variables: {},
    nodes: [node("design", 100, 3), node("review", 300)],
    edges: [],
    notes: [{ id: "n1", content: "note", view: { x: 300, y: 300 } }],
  };
}

const stored = () => useEditStore.getState().openTabs[0].pipeline;
const drawnY = (p: EditorPipelineDef, id: string) => grownCanvas(p).pipeline.nodes.find((n) => n.id === id)!.view!.y;

beforeEach(() => {
  useEditStore.setState({
    openTabs: [{ id: "tab1", scope: "repo", pipeline: pipeline(), prompts: {}, diagnostics: [], dirty: false, externalDirty: false }],
    activeTabId: "tab1",
    selection: { kind: "none", id: null },
  });
});

describe("a drag on a grown canvas writes the stored position", () => {
  it("stores a card where it is drawn from, so it is drawn where it was dropped", () => {
    const shift = grownCanvas(stored());
    const { result } = renderHook(() => useCanvasNodeDragStop(shift));
    const dropped = { id: "review", type: "edit", position: { x: 40, y: 400 }, data: {} } as Node;
    result.current(null, dropped, [dropped]);
    expect(stored().nodes.find((n) => n.id === "review")!.view).toEqual({ x: 40, y: 368 });
    expect(drawnY(stored(), "review")).toBe(400);
  });

  it("stores a dragged note the same way", () => {
    const shift = grownCanvas(stored());
    const { result } = renderHook(() => useCanvasNodeDragStop(shift));
    const dropped = { id: "n1", type: "note", position: { x: 300, y: 450 }, data: {} } as Node;
    result.current(null, dropped, [dropped]);
    expect(stored().notes![0].view).toEqual({ x: 300, y: 418 });
  });

  it("stores the drawn position as is when nothing grows", () => {
    const { result } = renderHook(() => useCanvasNodeDragStop());
    const dropped = { id: "review", type: "edit", position: { x: 0, y: 400 }, data: {} } as Node;
    result.current(null, dropped, [dropped]);
    expect(stored().nodes.find((n) => n.id === "review")!.view!.y).toBe(400);
  });
});

describe("storedEdgePatch", () => {
  it("takes waypoints and pinned labels back to where they are stored", () => {
    const shift = grownCanvas(pipeline());
    expect(
      storedEdgePatch(shift, {
        mode: "manual",
        waypoints: [
          { x: 10, y: 132 },
          { x: 10, y: 400 },
        ],
        condition_label_pos: { x: 5, y: 400 },
        output_label_pos: { out: { x: 1, y: 420 } },
      }),
    ).toEqual({
      mode: "manual",
      waypoints: [
        { x: 10, y: 132 },
        { x: 10, y: 368 },
      ],
      condition_label_pos: { x: 5, y: 368 },
      output_label_pos: { out: { x: 1, y: 388 } },
    });
  });

  it("leaves an update untouched when nothing is shifted", () => {
    const updates = { waypoints: [{ x: 1, y: 400 }] };
    expect(storedEdgePatch(IDENTITY_SHIFT, updates)).toBe(updates);
  });
});
