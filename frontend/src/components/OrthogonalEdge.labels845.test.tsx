import { render, act, fireEvent } from "@testing-library/react";
import { describe, it, expect, beforeEach, vi } from "vitest";
import { ReactFlowProvider } from "@xyflow/react";
import OrthogonalEdge, { type OrthogonalEdgeData } from "./OrthogonalEdge";
import { useEditStore } from "../stores/editStore";
import type { EdgeDef, PipelineDef } from "../types";

// #845 — the two kinds of canvas label: the output tags at the arrow's base and
// the condition pill at its midpoint. Both are draggable, neither may block a
// click on the edge underneath.

// `EdgeLabelRenderer` portals into the `.react-flow__edgelabel-renderer` div of
// a mounted <ReactFlow>, which doesn't exist under a bare provider. Render its
// children inline so the labels land in the test container.
vi.mock("@xyflow/react", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@xyflow/react")>();
  return {
    ...actual,
    EdgeLabelRenderer: ({ children }: { children: React.ReactNode }) => <>{children}</>,
  };
});

function Wrapper({ children }: { children: React.ReactNode }) {
  return <ReactFlowProvider>{children}</ReactFlowProvider>;
}

/** A one-edge pipeline in the store: the labels write back through `updateEdge`. */
function seedEdge(edge: Partial<EdgeDef> = {}) {
  const pipeline: PipelineDef = {
    name: "labels-845",
    variables: {},
    nodes: [
      {
        id: "design",
        name: "design",
        type: "agent",
        inputs: [],
        outputs: [
          { name: "out", repeated: false, side: "right" },
          { name: "spec", repeated: false, side: "right" },
        ],
        interactive: false,
      },
      {
        id: "orch",
        name: "orch",
        type: "agent",
        inputs: [],
        outputs: [],
        interactive: false,
      },
    ],
    edges: [
      {
        source: { node: "design", ports: ["out", "spec"] },
        target: { node: "orch", port: "out" },
        ...edge,
      },
    ],
  };
  useEditStore.setState({
    openTabs: [
      {
        id: "tab1",
        scope: "repo",
        pipeline,
        prompts: {},
        diagnostics: [],
        dirty: false,
        externalDirty: false,
      },
    ],
    activeTabId: "tab1",
    selection: { kind: "none", id: null },
  });
}

function currentEdge(): EdgeDef {
  return useEditStore.getState().openTabs[0].pipeline.edges[0];
}

function edgeProps(data?: Partial<OrthogonalEdgeData>) {
  return {
    id: "e-0",
    source: "design",
    target: "orch",
    sourceX: 0,
    sourceY: 0,
    targetX: 200,
    targetY: 0,
    markerEnd: "",
    data: {
      edgeIndex: 0,
      mode: null,
      waypoints: null,
      isConditional: false,
      isElse: false,
      strokeColor: "var(--color-fg-4)",
      dashed: false,
      ports: ["out", "spec"],
      showOutputLabels: true,
      outputLabelPos: null,
      conditionLabelPos: null,
      ...data,
    },
  } as unknown as Parameters<typeof OrthogonalEdge>[0];
}

function label(container: HTMLElement, testId: string): HTMLElement {
  const el = container.querySelector<HTMLElement>(`[data-testid="${testId}"]`);
  if (!el) throw new Error(`label ${testId} not found`);
  return el;
}

/** The `translate(<x>px, <y>px)` pair a label is positioned with. */
function labelPoint(el: HTMLElement): { x: number; y: number } {
  const m = /translate\((-?[\d.]+)px,\s*(-?[\d.]+)px\)/.exec(el.style.transform);
  if (!m) throw new Error(`no absolute translate in "${el.style.transform}"`);
  return { x: Number(m[1]), y: Number(m[2]) };
}

/** A press-move-release gesture on a label, in screen coordinates. */
function drag(el: HTMLElement, from: { x: number; y: number }, to: { x: number; y: number }) {
  fireEvent.pointerDown(el, { button: 0, clientX: from.x, clientY: from.y });
  act(() => {
    window.dispatchEvent(new MouseEvent("pointermove", { clientX: to.x, clientY: to.y }));
    window.dispatchEvent(new MouseEvent("pointerup", {}));
  });
}

beforeEach(() => {
  useEditStore.setState({
    openTabs: [],
    activeTabId: null,
    selection: { kind: "none", id: null },
  });
});

describe("output labels (#845)", () => {
  it("names every carried port at the arrow's base", () => {
    seedEdge();
    const { container } = render(<OrthogonalEdge {...edgeProps()} />, { wrapper: Wrapper });

    expect(label(container, "edge-output-label-e-0-out")).toHaveTextContent("out");
    expect(label(container, "edge-output-label-e-0-spec")).toHaveTextContent("spec");
  });

  it("renders nothing when the toggle is off", () => {
    seedEdge();
    const { container } = render(
      <OrthogonalEdge {...edgeProps({ showOutputLabels: false })} />,
      { wrapper: Wrapper },
    );
    expect(container.querySelectorAll('[data-testid^="edge-output-label-"]')).toHaveLength(0);
  });

  it("alternates around the base: the first above the stroke, the second below", () => {
    seedEdge();
    const { container } = render(<OrthogonalEdge {...edgeProps()} />, { wrapper: Wrapper });

    const first = labelPoint(label(container, "edge-output-label-e-0-out"));
    const second = labelPoint(label(container, "edge-output-label-e-0-spec"));
    expect(first.y).toBeLessThan(0);
    expect(second.y).toBeGreaterThan(0);
    // Both just past the source rim, on the same side of it (the edge leaves
    // rightwards), and anchored by their near edge so a long name grows away
    // from the card.
    expect(first.x).toBeGreaterThan(0);
    expect(second.x).toBe(first.x);
    expect(label(container, "edge-output-label-e-0-out").style.transform).toContain(
      "translate(0%, -50%)",
    );
  });

  it("sits where it was dropped once pinned, centred on its own position", () => {
    seedEdge();
    const { container } = render(
      <OrthogonalEdge {...edgeProps({ outputLabelPos: { spec: { x: 400, y: 300 } } })} />,
      { wrapper: Wrapper },
    );

    const el = label(container, "edge-output-label-e-0-spec");
    expect(labelPoint(el)).toEqual({ x: 400, y: 300 });
    expect(el.style.transform).toContain("translate(-50%, -50%)");
    // The un-pinned sibling keeps its default spot.
    expect(labelPoint(label(container, "edge-output-label-e-0-out")).x).toBeLessThan(400);
  });

  it("is grabbable — the label layer is pointer-events:none, children must opt back in", () => {
    seedEdge();
    const { container } = render(<OrthogonalEdge {...edgeProps()} />, { wrapper: Wrapper });
    expect(label(container, "edge-output-label-e-0-out").style.pointerEvents).toBe("all");
  });

  it("writes the dragged position, per port, keeping its sibling's", () => {
    seedEdge({ output_label_pos: { out: { x: 5, y: 6 } } });
    const { container } = render(
      <OrthogonalEdge {...edgeProps({ outputLabelPos: { out: { x: 5, y: 6 } } })} />,
      { wrapper: Wrapper },
    );

    drag(label(container, "edge-output-label-e-0-spec"), { x: 10, y: 10 }, { x: 120, y: 240 });

    expect(currentEdge().output_label_pos).toEqual({
      out: { x: 5, y: 6 },
      spec: { x: 120, y: 240 },
    });
  });

  it("hands a plain click to the edge instead of swallowing it", () => {
    // "Labels must not block clicks on the edge": below the drag threshold the
    // press selects the edge, exactly as a click on the stroke would.
    seedEdge();
    const { container } = render(<OrthogonalEdge {...edgeProps()} />, { wrapper: Wrapper });

    const el = label(container, "edge-output-label-e-0-out");
    fireEvent.pointerDown(el, { button: 0, clientX: 40, clientY: 40 });
    act(() => {
      window.dispatchEvent(new MouseEvent("pointermove", { clientX: 41, clientY: 41 }));
      window.dispatchEvent(new MouseEvent("pointerup", {}));
    });

    expect(useEditStore.getState().selection).toMatchObject({ kind: "edge", edgeIndex: 0 });
    expect(currentEdge().output_label_pos).toBeUndefined();
  });

  it("ignores a right-press: the context-menu gesture must not move the label", () => {
    seedEdge();
    const { container } = render(<OrthogonalEdge {...edgeProps()} />, { wrapper: Wrapper });

    drag(label(container, "edge-output-label-e-0-out"), { x: 10, y: 10 }, { x: 300, y: 300 });
    // (the helper presses with button 0; repeat with the right button)
    const el = label(container, "edge-output-label-e-0-spec");
    fireEvent.pointerDown(el, { button: 2, clientX: 10, clientY: 10 });
    act(() => {
      window.dispatchEvent(new MouseEvent("pointermove", { clientX: 300, clientY: 300 }));
      window.dispatchEvent(new MouseEvent("pointerup", {}));
    });

    expect(currentEdge().output_label_pos).toHaveProperty("out");
    expect(currentEdge().output_label_pos).not.toHaveProperty("spec");
  });
});

describe("condition label (#845)", () => {
  const conditional = { label: "out.has_design_work = true" };

  it("sits clear of the stroke, not on it — the midpoint handle must stay grabbable", () => {
    seedEdge();
    const { container } = render(<OrthogonalEdge {...edgeProps(conditional)} />, {
      wrapper: Wrapper,
    });

    const pill = label(container, "edge-condition-label-e-0");
    expect(pill).toHaveTextContent("out.has_design_work = true");
    // The path runs along y = 0, so a pill centred on the stroke would be at 0.
    expect(labelPoint(pill).y).toBeLessThan(0);
    expect(pill.style.pointerEvents).toBe("all");
  });

  it("looks unlike an output tag — pill border and radius against a flat square tag", () => {
    seedEdge();
    const { container } = render(<OrthogonalEdge {...edgeProps(conditional)} />, {
      wrapper: Wrapper,
    });

    const pill = label(container, "edge-condition-label-e-0");
    const tag = label(container, "edge-output-label-e-0-out");
    expect(pill.style.borderRadius).not.toBe(tag.style.borderRadius);
    expect(pill.style.background).not.toBe(tag.style.background);
    // The pill is outlined in the edge's own colour; the tag is a hairline.
    expect(pill.style.border).toContain("--color-fg-4");
    expect(tag.style.border).toContain("--color-line");
  });

  it("writes its dragged position", () => {
    seedEdge();
    const { container } = render(<OrthogonalEdge {...edgeProps(conditional)} />, {
      wrapper: Wrapper,
    });

    drag(label(container, "edge-condition-label-e-0"), { x: 0, y: 0 }, { x: 77, y: 88 });

    expect(currentEdge().condition_label_pos).toEqual({ x: 77, y: 88 });
  });

  it("sits at its pinned position when it has one", () => {
    seedEdge();
    const { container } = render(
      <OrthogonalEdge {...edgeProps({ ...conditional, conditionLabelPos: { x: 12, y: 34 } })} />,
      { wrapper: Wrapper },
    );
    expect(labelPoint(label(container, "edge-condition-label-e-0"))).toEqual({ x: 12, y: 34 });
  });

  it("hands a plain click to the edge, like the output tags", () => {
    seedEdge();
    const { container } = render(<OrthogonalEdge {...edgeProps(conditional)} />, {
      wrapper: Wrapper,
    });

    fireEvent.pointerDown(label(container, "edge-condition-label-e-0"), {
      button: 0,
      clientX: 20,
      clientY: 20,
    });
    act(() => {
      window.dispatchEvent(new MouseEvent("pointerup", {}));
    });

    expect(useEditStore.getState().selection).toMatchObject({ kind: "edge", edgeIndex: 0 });
    expect(currentEdge().condition_label_pos).toBeUndefined();
  });
});
