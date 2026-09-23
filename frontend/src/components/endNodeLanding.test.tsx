import { render } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import { ReactFlowProvider } from "@xyflow/react";
import { TooltipProvider } from "./ui/tooltip";
import { EditNode } from "./EditCanvas";
import { deriveEditEdges } from "./editNodeDerivation";
import { landsByDrop } from "../lib/anchorSide";
import { droppedPath, hoverLanding, startGesture, type HoveredHandle, type HoveredNode } from "../lib/wiringGesture";
import { WIRING_GRID_STEP } from "../lib/wiringGrid";
import type { EdgeAnchor, NodeDef, NodeStatus, PipelineDef, PortSide } from "../types";

// #840 (human test): an edge could land on the End marker by its LEFT border
// only. End kept its declared `result` handle, pinned on the declared side
// (`side: left` in every real pipeline), so the landing preview, the saved anchor
// and the rendered arrow all ignored where the wire was dropped. End is a target
// like any other now: the arrow lands on the border it was dropped on.

const SIDES: PortSide[] = ["top", "right", "bottom", "left"];

function endNode(declaredSide: PortSide = "left"): NodeDef {
  return {
    id: "end",
    name: "End",
    type: "end",
    inputs: [{ name: "result", repeated: false, side: declaredSide }],
    outputs: [],
    interactive: false,
  };
}

function pipelineToEnd(
  edge: Partial<PipelineDef["edges"][number]>,
  declaredSide: PortSide = "left",
): PipelineDef {
  return {
    name: "p",
    variables: {},
    nodes: [
      {
        id: "src",
        name: "src",
        type: "agent",
        inputs: [],
        outputs: [{ name: "plan", repeated: false, side: "right" }],
        interactive: false,
      },
      endNode(declaredSide),
    ],
    edges: [
      {
        source: { node: "src", port: "plan" },
        target: { node: "end", port: "result" },
        ...edge,
      },
    ],
  };
}

describe("End marker accepts an incoming edge on any of its four borders (#840)", () => {
  it("counts End among the targets that land where the wire is dropped", () => {
    expect(landsByDrop("end")).toBe(true);
    expect(landsByDrop("agent")).toBe(true);
    expect(landsByDrop("script")).toBe(true);
    // A merge keeps its `branches` port pill; Start takes no incoming edge.
    expect(landsByDrop("merge")).toBe(false);
    expect(landsByDrop("start")).toBe(false);
  });

  it.each(SIDES)("binds a saved End edge to the %s border it was dropped on", (side) => {
    const anchor: EdgeAnchor = { side, offset: 30 };
    const [edge] = deriveEditEdges(
      pipelineToEnd({ target_anchor: anchor, ...(side === "left" ? {} : { target_side: side }) }),
    );
    // The handle xyflow binds to AND the side the edge squares its landing leg
    // against: both must be the dropped side, or the arrow ends parallel to it.
    expect(edge.targetHandle).toBe(`__anchor:${side}`);
    expect(edge.data?.targetSide).toBe(side);
    expect(edge.data?.targetAnchor).toEqual(anchor);
  });

  it("lets the saved anchor win over the declared side when the drop was on the left", () => {
    // `target_side: left` is never written (it is the default), so a left drop on
    // an End declared `top` survives only through its anchor.
    const [edge] = deriveEditEdges(
      pipelineToEnd({ target_anchor: { side: "left", offset: 20 } }, "top"),
    );
    expect(edge.targetHandle).toBe("__anchor:left");
    expect(edge.data?.targetSide).toBe("left");
  });

  it.each(SIDES)("keeps a legacy End edge (no anchor) on its declared %s side", (declared) => {
    // Pipelines saved before this fix carry no anchor on their End edges: they
    // reopen exactly where they used to land, on `result`'s declared side.
    const [edge] = deriveEditEdges(pipelineToEnd({}, declared));
    expect(edge.targetHandle).toBe(`__anchor:${declared}`);
    expect(edge.data?.targetSide).toBe(declared);
  });

  it("keeps carrying the declared `result` port", () => {
    const [edge] = deriveEditEdges(pipelineToEnd({ target_anchor: { side: "top", offset: 30 } }));
    expect(edge.target).toBe("end");
    // The port is semantics, the handle is layout: the port name is untouched.
    expect(pipelineToEnd({}).edges[0].target.port).toBe("result");
  });

  describe("landing preview and drop", () => {
    // End card at (400, 400), 160 x 40 — the default marker size.
    const RECT = { x: 400, y: 400, width: 160, height: 40 };
    const END: HoveredNode = {
      id: "end",
      measured: { width: RECT.width, height: RECT.height },
      internals: { positionAbsolute: { x: RECT.x, y: RECT.y } },
      data: { nodeType: "end" },
    };
    // What xyflow hands over while the pointer is over the card: the full-card
    // body handle, whose `position` is the declared side (`left`).
    const BODY: HoveredHandle = {
      x: RECT.x + RECT.width / 2,
      y: RECT.y + RECT.height / 2,
      width: RECT.width,
      height: RECT.height,
      position: "left",
    };
    const AIM: Record<PortSide, { x: number; y: number }> = {
      top: { x: 470, y: 403 },
      right: { x: 556, y: 420 },
      bottom: { x: 470, y: 437 },
      left: { x: 404, y: 420 },
    };
    const BORDER: Record<PortSide, (p: { x: number; y: number }) => boolean> = {
      top: (p) => p.y === RECT.y,
      right: (p) => p.x === RECT.x + RECT.width,
      bottom: (p) => p.y === RECT.y + RECT.height,
      left: (p) => p.x === RECT.x,
    };

    it.each(SIDES)("previews the landing on the %s border aimed at", (side) => {
      const landing = hoverLanding(AIM[side], END, BODY, "src");
      expect(landing?.anchor.side).toBe(side);
      expect(BORDER[side](landing!.point)).toBe(true);
    });

    it.each(SIDES)("saves a route ending perpendicular into the %s border", (side) => {
      const gesture = startGesture({ x: 80, y: 80 }, "bottom", WIRING_GRID_STEP);
      const traced = droppedPath(gesture, AIM[side], END, BODY, "src", WIRING_GRID_STEP);
      const [before, last] = traced.slice(-2);
      expect(BORDER[side](last)).toBe(true);
      // The last leg runs along the side's normal: vertical into top/bottom,
      // horizontal into left/right.
      if (side === "top" || side === "bottom") expect(before.x).toBe(last.x);
      else expect(before.y).toBe(last.y);
    });
  });

  it("renders the four per-side landing handles on the End card", () => {
    const props = {
      id: "end",
      type: "edit",
      selected: false,
      dragging: false,
      zIndex: 0,
      isConnectable: true,
      positionAbsoluteX: 0,
      positionAbsoluteY: 0,
      data: {
        label: "End",
        nodeId: "end",
        nodeType: "end",
        status: "pending" as NodeStatus,
        reached: false,
        inputs: [{ name: "result", side: "left" as const }],
        outputs: [],
        interactive: false,
      },
    } as unknown as Parameters<typeof EditNode>[0];
    const { container } = render(
      <TooltipProvider>
        <ReactFlowProvider>
          <EditNode {...props} />
        </ReactFlowProvider>
      </TooltipProvider>,
    );
    const handles = Array.from(container.querySelectorAll(".react-flow__handle"));
    for (const side of SIDES) {
      const h = handles.find((el) => el.getAttribute("data-handleid") === `__anchor:${side}`);
      expect(h, `__anchor:${side}`).toBeTruthy();
      expect(h!.getAttribute("data-handlepos")).toBe(side);
    }
    // End still starts no wire: no output, no rim.
    expect(handles.some((el) => el.getAttribute("data-handleid")?.startsWith("rim-"))).toBe(false);
  });
});
