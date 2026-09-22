import { render, fireEvent } from "@testing-library/react";
import { describe, it, expect, beforeEach, afterEach } from "vitest";
import { Handle, Position, ReactFlow, ReactFlowProvider, type Edge, type Node } from "@xyflow/react";
import OrthogonalEdge, { type OrthogonalEdgeData } from "./OrthogonalEdge";
import NodeRimHandles from "./NodeRimHandles";
import { anchorHandleId } from "../lib/anchorSide";
import { useEditStore } from "../stores/editStore";
import type { PipelineDef, PortSide } from "../types";

// xyflow MEASURES before it draws: a node with no measured size and no handle
// bounds yields no edge position, and `react-flow__edges` comes out empty — the
// component under test never renders at all. jsdom lays nothing out, so the three
// things xyflow reads are stubbed here (in this file only, so no other suite's
// layout behaviour shifts under it): a ResizeObserver that actually fires,
// `offsetWidth/Height`, and `getBoundingClientRect`.
class FiringResizeObserver {
  cb: ResizeObserverCallback;
  constructor(cb: ResizeObserverCallback) {
    this.cb = cb;
  }
  observe(target: Element) {
    this.cb([{ target } as ResizeObserverEntry], this as unknown as ResizeObserver);
  }
  unobserve() {}
  disconnect() {}
}
(globalThis as Record<string, unknown>).ResizeObserver = FiringResizeObserver;

// xyflow reads the viewport's CSS transform through `DOMMatrixReadOnly`, which
// jsdom does not implement. The tests run unpanned at zoom 1, so identity is the
// honest answer.
(globalThis as Record<string, unknown>).DOMMatrixReadOnly = class {
  m11 = 1; m12 = 0; m13 = 0; m14 = 0;
  m21 = 0; m22 = 1; m23 = 0; m24 = 0;
  m31 = 0; m32 = 0; m33 = 1; m34 = 0;
  m41 = 0; m42 = 0; m43 = 0; m44 = 1;
  a = 1; b = 0; c = 0; d = 1; e = 0; f = 0;
};

for (const [prop, value] of [["offsetWidth", 200], ["offsetHeight", 80]] as const) {
  Object.defineProperty(HTMLElement.prototype, prop, { configurable: true, value });
}
HTMLElement.prototype.getBoundingClientRect = function (this: HTMLElement) {
  const w = this.classList.contains("react-flow__handle") ? 1 : 200;
  const h = this.classList.contains("react-flow__handle") ? 1 : 80;
  return { x: 0, y: 0, top: 0, left: 0, right: w, bottom: h, width: w, height: h, toJSON: () => ({}) };
};

// Two cards 400px apart, wired bottom → top by hand: the geometry #844 draws.
const SRC = { x: 0, y: 0, width: 200, height: 80 };
const TGT = { x: 400, y: 400, width: 200, height: 80 };

// A stand-in for `EditNode`, carrying exactly the handles an edge binds to: the
// four rim drag-sources and the four body anchors. xyflow drops an edge whose
// handle id matches nothing rendered (error 008), so these have to be real.
const SIDES: { side: PortSide; position: Position }[] = [
  { side: "left", position: Position.Left },
  { side: "right", position: Position.Right },
  { side: "top", position: Position.Top },
  { side: "bottom", position: Position.Bottom },
];

function TestCard() {
  return (
    <div style={{ width: 200, height: 80 }}>
      {SIDES.map(({ side, position }) => (
        <Handle
          key={side}
          id={anchorHandleId(side)}
          type="target"
          position={position}
          isConnectableStart={false}
        />
      ))}
      <NodeRimHandles />
    </div>
  );
}

const nodeTypes = { card: TestCard };

const nodes: Node[] = [
  { id: "src", type: "card", position: { x: SRC.x, y: SRC.y }, width: SRC.width, height: SRC.height, data: {} },
  { id: "tgt", type: "card", position: { x: TGT.x, y: TGT.y }, width: TGT.width, height: TGT.height, data: {} },
];

function pipeline(edge: Partial<PipelineDef["edges"][number]> = {}): PipelineDef {
  return {
    name: "p",
    variables: {},
    nodes: [
      { id: "src", name: "src", type: "agent", inputs: [], outputs: [{ name: "out", repeated: false }], interactive: false },
      { id: "tgt", name: "tgt", type: "agent", inputs: [], outputs: [], interactive: false },
    ],
    edges: [
      {
        source: { node: "src", port: "out" },
        target: { node: "tgt", port: "out" },
        mode: "manual",
        waypoints: [{ x: 80, y: 240 }, { x: 480, y: 240 }],
        target_side: "top",
        source_anchor: { side: "bottom", offset: 80 },
        target_anchor: { side: "top", offset: 80 },
        ...edge,
      },
    ],
  };
}

function edgeData(overrides: Partial<OrthogonalEdgeData> = {}): OrthogonalEdgeData {
  return {
    edgeIndex: 0,
    mode: "manual",
    waypoints: [{ x: 80, y: 240 }, { x: 480, y: 240 }],
    targetSide: "top",
    sourceAnchor: { side: "bottom", offset: 80 },
    targetAnchor: { side: "top", offset: 80 },
    isConditional: false,
    isElse: false,
    strokeColor: "var(--color-fg-4)",
    dashed: false,
    ...overrides,
  };
}

function harness(data: OrthogonalEdgeData = edgeData()) {
  const edges: Edge<OrthogonalEdgeData>[] = [
    {
      id: "e-0",
      source: "src",
      target: "tgt",
      sourceHandle: "rim-bottom",
      targetHandle: "__anchor:top",
      type: "orthogonal",
      data,
    },
  ];
  return render(
    <ReactFlowProvider>
      <div style={{ width: 800, height: 800 }}>
        <ReactFlow
          nodes={nodes}
          edges={edges}
          nodeTypes={nodeTypes}
          edgeTypes={{ orthogonal: OrthogonalEdge }}
          proOptions={{ hideAttribution: true }}
        />
      </div>
    </ReactFlowProvider>,
  );
}

/** The `d` of the drawn stroke, parsed back into points. */
function drawnPoints(): { x: number; y: number }[] {
  const path = document.querySelector('[data-testid="orthogonal-edge-hit-e-0"]') as SVGPathElement;
  return (path.getAttribute("d") ?? "")
    .split(/[ML]\s*/)
    .filter(Boolean)
    .map((pair) => {
      const [x, y] = pair.trim().split(",").map(Number);
      return { x, y };
    });
}

beforeEach(() => {
  useEditStore.setState({
    openTabs: [
      {
        id: "t1",
        name: "p",
        scope: "library",
        pipeline: pipeline(),
        prompts: {},
        dirty: false,
      },
    ] as never,
    activeTabId: "t1",
    selection: { kind: "edge", id: null, edgeIndex: 0 },
  });
});

afterEach(() => {
  useEditStore.setState({ selection: { kind: "none", id: null } });
});

describe("OrthogonalEdge — anchored geometry (#844)", () => {
  it("draws from the persisted anchors, not from the handle centres", () => {
    harness();
    const pts = drawnPoints();
    // source_anchor {bottom, 80} on a card at (0,0) 200x80 ⇒ (80, 80).
    expect(pts[0]).toEqual({ x: 80, y: 80 });
    // target_anchor {top, 80} on a card at (400,400) ⇒ (480, 400).
    expect(pts[pts.length - 1]).toEqual({ x: 480, y: 400 });
  });

  it("leaves and arrives perpendicular, so the arrowhead enters the card straight", () => {
    harness();
    const pts = drawnPoints();
    // First leg: vertical, downwards out of the bottom border.
    expect(pts[1].x).toBe(pts[0].x);
    expect(pts[1].y).toBeGreaterThan(pts[0].y);
    // Last leg: vertical, downwards into the top border.
    const last = pts[pts.length - 1];
    const before = pts[pts.length - 2];
    expect(before.x).toBe(last.x);
    expect(last.y).toBeGreaterThan(before.y);
  });

  it("gives the two structural end legs no drag handle", () => {
    harness();
    const pts = drawnPoints();
    const handles = document.querySelectorAll('[data-testid^="edge-seg-handle-e-0-"]');
    const indices = Array.from(handles).map((h) =>
      Number(h.getAttribute("data-testid")!.replace("edge-seg-handle-e-0-", "")),
    );
    expect(indices).not.toContain(0);
    expect(indices).not.toContain(pts.length - 2);
    expect(indices.length).toBeGreaterThan(0);
  });

  it("shows no handle at all while the edge is not selected (#178)", () => {
    useEditStore.setState({ selection: { kind: "none", id: null } });
    harness();
    expect(document.querySelectorAll('[data-testid^="edge-seg-handle-"]')).toHaveLength(0);
  });
});

describe("OrthogonalEdge — right-click on a segment handle does nothing (#844)", () => {
  it("leaves the edge untouched and swallows the event so no menu opens", () => {
    harness();
    const before = JSON.stringify(useEditStore.getState().openTabs[0].pipeline.edges[0]);
    const handle = document.querySelectorAll('[data-testid^="edge-seg-handle-e-0-"]')[0];

    const menu = fireEvent.contextMenu(handle);
    // `fireEvent` returns false when a handler called `preventDefault` — which is
    // what keeps the portalled React event from bubbling into xyflow's own
    // `onEdgeContextMenu` and quietly opening the edge menu.
    expect(menu).toBe(false);

    // A right-click must not start a drag either: the gesture that used to delete
    // a waypoint is GONE, not renamed.
    fireEvent.pointerDown(handle, { button: 2 });
    fireEvent.pointerMove(window, { clientX: 300, clientY: 300 });
    fireEvent.pointerUp(window);

    expect(JSON.stringify(useEditStore.getState().openTabs[0].pipeline.edges[0])).toBe(before);
  });

  it("still lets the primary button reshape the route — the drag is not what went away", () => {
    harness();
    const before = JSON.stringify(useEditStore.getState().openTabs[0].pipeline.edges[0].waypoints);
    const handle = document.querySelectorAll('[data-testid^="edge-seg-handle-e-0-"]')[0];
    fireEvent.pointerDown(handle, { button: 0 });
    fireEvent.pointerMove(window, { clientX: 300, clientY: 300 });
    fireEvent.pointerUp(window);
    const edge = useEditStore.getState().openTabs[0].pipeline.edges[0];
    expect(edge.mode).toBe("manual");
    expect(JSON.stringify(edge.waypoints)).not.toBe(before);
  });
});

describe("OrthogonalEdge — an edge drawn before #844", () => {
  it("falls back to the handle centres when it carries no anchor", () => {
    harness(edgeData({ sourceAnchor: null, targetAnchor: null, mode: "auto", waypoints: null }));
    const pts = drawnPoints();
    expect(pts.length).toBeGreaterThanOrEqual(2);
    // Whatever xyflow reports, the path is still orthogonal end to end.
    for (let i = 1; i < pts.length; i++) {
      const square = pts[i].x === pts[i - 1].x || pts[i].y === pts[i - 1].y;
      expect(square).toBe(true);
    }
  });
});

describe("OrthogonalEdge — an un-anchored edge leaves by its output's declared side (#844 FP iter-2)", () => {
  it("heads down out of a `side: bottom` output instead of rightwards", () => {
    harness(
      edgeData({
        sourceAnchor: null,
        targetAnchor: null,
        sourceSide: "bottom",
        mode: "auto",
        waypoints: null,
      }),
    );
    const pts = drawnPoints();
    // First leg: vertical and downwards — out of the bottom border, like the
    // output dot that used to sit there.
    expect(pts[1].x).toBe(pts[0].x);
    expect(pts[1].y).toBeGreaterThan(pts[0].y);
  });

  it("keeps the rightwards departure when nothing says otherwise", () => {
    harness(edgeData({ sourceAnchor: null, targetAnchor: null, mode: "auto", waypoints: null }));
    const pts = drawnPoints();
    expect(pts[1].y).toBe(pts[0].y);
    expect(pts[1].x).toBeGreaterThan(pts[0].x);
  });
});
