import { render, screen, within, fireEvent } from "@testing-library/react";
import { describe, it, expect, afterEach } from "vitest";
import { ReactFlowProvider } from "@xyflow/react";
import { TooltipProvider } from "./ui/tooltip";
import { EditNode } from "./EditCanvas";
import { useEditStore } from "../stores/editStore";
import { deriveEditEdges } from "./editNodeDerivation";
import { landsByDrop } from "../lib/anchorSide";
import { droppedPath, hoverLanding, startGesture, type HoveredHandle, type HoveredNode } from "../lib/wiringGesture";
import { gridStep } from "../lib/wiringGrid";

// Fixtures laid out on the 40px (L) lattice (#877: the step is a parameter).
const GRID_STEP = gridStep("L");
import type { EdgeAnchor, NodeDef, NodeStatus, NodeType, PipelineDef, PortSide } from "../types";

// EditNode reads selection from the global edit store; reset it between tests so
// a marker is never accidentally rendered "selected".
afterEach(() => {
  useEditStore.getState().setSelection({ kind: "none", id: null });
});

function Wrapper({ children }: { children: React.ReactNode }) {
  return (
    <TooltipProvider>
      <ReactFlowProvider>{children}</ReactFlowProvider>
    </TooltipProvider>
  );
}

interface MarkerOpts {
  nodeType: NodeType;
  reached?: boolean;
  status?: NodeStatus;
  inputImages?: string[];
}

function markerProps({ nodeType, reached = false, status = "pending", inputImages }: MarkerOpts) {
  const data = {
    label: nodeType,
    nodeId: nodeType,
    nodeType,
    status,
    reached,
    inputImages,
    inputs: nodeType === "end" ? [{ name: "in", side: "left" as const }] : [],
    outputs: nodeType === "start" ? [{ name: "out", side: "right" as const }] : [],
    interactive: false,
  };
  return {
    id: nodeType,
    data,
    type: "edit",
    selected: false,
    isConnectable: true,
    zIndex: 0,
    positionAbsoluteX: 0,
    positionAbsoluteY: 0,
    dragging: false,
    deletable: true,
    selectable: true,
    parentId: undefined,
    dragHandle: undefined,
    sourcePosition: undefined,
    targetPosition: undefined,
    width: 160,
    height: 80,
  } as unknown as Parameters<typeof EditNode>[0];
}

describe("EditNode start/end markers — green-on-complete (issue #105, inline run view)", () => {
  it("Start: neutral card cadre with a green play icon before the run completes", () => {
    render(<EditNode {...markerProps({ nodeType: "start", reached: false })} />, { wrapper: Wrapper });
    const card = screen.getByTestId("node-card");
    // Non-completed baseline in the inline view is the neutral "pending" cadre.
    expect(card.className).toContain("border-line-strong");
    expect(card.className).toContain("bg-bg-3");
    expect(card.className).not.toContain("border-st-done");
    expect(screen.getByTestId("node-icon-start").getAttribute("class")).toContain("text-acc");
  });

  it("Start: borrows the green completed cadre once the run reaches its end", () => {
    render(<EditNode {...markerProps({ nodeType: "start", reached: true })} />, { wrapper: Wrapper });
    const card = screen.getByTestId("node-card");
    expect(card.className).toContain("border-st-done");
    expect(card.className).toContain("bg-st-done-bg");
    expect(screen.getByTestId("node-icon-start").getAttribute("class")).toContain("text-st-done");
  });

  it("End: neutral card cadre with an orange square icon before the run completes", () => {
    render(<EditNode {...markerProps({ nodeType: "end", reached: false })} />, { wrapper: Wrapper });
    const card = screen.getByTestId("node-card");
    expect(card.className).toContain("border-line-strong");
    expect(card.className).not.toContain("border-st-done");
    // Bug-report correction: in the inline view the non-completed End card border
    // is neutral grey, not orange — only the icon is orange.
    expect(screen.getByTestId("node-icon-end").getAttribute("class")).toContain("text-st-blocked");
  });

  it("End: turns green (border + faint green fill + green icon) once the run reaches its end", () => {
    render(<EditNode {...markerProps({ nodeType: "end", reached: true })} />, { wrapper: Wrapper });
    const card = screen.getByTestId("node-card");
    expect(card.className).toContain("border-st-done");
    expect(card.className).toContain("bg-st-done-bg");
    const icon = screen.getByTestId("node-icon-end").getAttribute("class");
    expect(icon).toContain("text-st-done");
    expect(icon).not.toContain("text-st-blocked");
  });
});

describe("EditNode slim card (issue #149)", () => {
  function workProps(overrides: Partial<{ interactive: boolean; isolated: boolean }> = {}) {
    const data = {
      label: "rewrite_section",
      nodeId: "nd_4f2a",
      nodeType: "agent" as NodeType,
      status: "pending" as NodeStatus,
      reached: false,
      inputs: [],
      outputs: [{ name: "out", side: "right" as const }],
      interactive: overrides.interactive ?? false,
      isolated: overrides.isolated ?? false,
    };
    return {
      ...markerProps({ nodeType: "agent" }),
      id: "nd_4f2a",
      data,
    } as unknown as Parameters<typeof EditNode>[0];
  }

  it("shows the node name and the isolation marker (#653)", () => {
    render(<EditNode {...workProps({ isolated: true })} />, { wrapper: Wrapper });
    expect(screen.getByText("rewrite_section")).toBeTruthy();
    expect(screen.getByTestId("isolation-marker")).toBeTruthy();
  });

  it("leaves a shared-worktree node unmarked (#653)", () => {
    render(<EditNode {...workProps({ isolated: false })} />, { wrapper: Wrapper });
    expect(screen.queryByTestId("isolation-marker")).toBeNull();
  });

  it("does not render the node id on the card", () => {
    const { container } = render(<EditNode {...workProps()} />, { wrapper: Wrapper });
    // The slim card drops the node id (#149).
    expect(container.textContent).not.toContain("nd_4f2a");
  });

  it("does not render the interactive badge on the card", () => {
    render(<EditNode {...workProps({ interactive: true })} />, { wrapper: Wrapper });
    // The amber interactive badge is removed from the card (#149).
    expect(screen.queryByText("interactive")).toBeNull();
  });

  it("renders no input dots (inputs are emergent — #149)", () => {
    const { container } = render(<EditNode {...workProps()} />, { wrapper: Wrapper });
    expect(container.querySelectorAll(".port-pill.kind-input")).toHaveLength(0);
  });

  it("renders a body-covering target handle on each side so an edge can anchor by drop position (#168)", () => {
    const { container } = render(<EditNode {...workProps()} />, { wrapper: Wrapper });
    const handleIds = Array.from(
      container.querySelectorAll(".react-flow__handle"),
    ).map((h) => h.getAttribute("data-handleid"));
    for (const side of ["left", "right", "top", "bottom"]) {
      expect(handleIds).toContain(`__anchor:${side}`);
    }
  });
});

describe("EditNode emergent anchoring keyed on node type (issue #175)", () => {
  function nodeProps(
    nodeType: NodeType,
    inputs: { name: string; side: "left" | "right" | "top" | "bottom" }[],
  ) {
    return {
      ...markerProps({ nodeType }),
      id: nodeType,
      data: {
        label: nodeType,
        nodeId: nodeType,
        nodeType,
        status: "pending" as NodeStatus,
        reached: false,
        inputs,
        // End has no outputs; a work node's outputs are irrelevant to anchoring.
        outputs: [],
        interactive: false,
      },
    } as unknown as Parameters<typeof EditNode>[0];
  }

  it("grows all four body anchor handles on a work node that still carries a vestigial declared `in`", () => {
    // Regression (#175): the old `inputs.length !== 1` gate suppressed the
    // anchors on every real work node (each carries one declared `in`), so no
    // edge could anchor by drop and they all snapped to the left. Anchoring is
    // keyed on node TYPE now, so the per-side anchors appear regardless.
    const { container } = render(
      <EditNode {...nodeProps("agent", [{ name: "in", side: "left" }])} />,
      { wrapper: Wrapper },
    );
    const handleIds = Array.from(container.querySelectorAll(".react-flow__handle")).map((h) =>
      h.getAttribute("data-handleid"),
    );
    for (const side of ["left", "right", "top", "bottom"]) {
      expect(handleIds).toContain(`__anchor:${side}`);
    }
  });

  it("lets the End marker land a wire on any border, whatever `result` declares (#840)", () => {
    // Until #840 End's `result` was a fixed-side handle and End grew no per-side
    // anchors: a wire could land on its declared side only. `result` is the
    // edge's port, not a place on the card — End anchors by drop like a work node.
    const { container } = render(
      <EditNode {...nodeProps("end", [{ name: "result", side: "top" }])} />,
      { wrapper: Wrapper },
    );
    const handleIds = Array.from(container.querySelectorAll(".react-flow__handle")).map((h) =>
      h.getAttribute("data-handleid"),
    );
    for (const side of ["left", "right", "top", "bottom"]) {
      expect(handleIds).toContain(`__anchor:${side}`);
    }
    expect(handleIds).not.toContain("result");
  });
});

describe("EditNode script node card (#248)", () => {
  function scriptProps() {
    return {
      ...markerProps({ nodeType: "script" }),
      id: "notify",
      data: {
        label: "notify",
        nodeId: "notify",
        nodeType: "script" as NodeType,
        status: "pending" as NodeStatus,
        reached: false,
        inputs: [],
        outputs: [{ name: "out", side: "right" as const }],
        interactive: false,
      },
    } as unknown as Parameters<typeof EditNode>[0];
  }

  it("renders the terminal (script) icon, not the agent glyph", () => {
    render(<EditNode {...scriptProps()} />, { wrapper: Wrapper });
    expect(screen.getByTestId("node-icon-script")).toBeTruthy();
    expect(screen.queryByTestId("node-icon-agent")).toBeNull();
  });

  it("grows all four body-anchor handles (emergent inputs, like a work node)", () => {
    const { container } = render(<EditNode {...scriptProps()} />, { wrapper: Wrapper });
    const handleIds = Array.from(container.querySelectorAll(".react-flow__handle")).map((h) =>
      h.getAttribute("data-handleid"),
    );
    for (const side of ["left", "right", "top", "bottom"]) {
      expect(handleIds).toContain(`__anchor:${side}`);
    }
  });
});

describe("EditNode Start marker — input images on the canvas (issue #145)", () => {
  it("renders one image chip per uploaded image, tagged by filename", () => {
    render(
      <EditNode {...markerProps({ nodeType: "start", inputImages: ["ui-bug.png", "trace.png"] })} />,
      { wrapper: Wrapper },
    );
    const strip = screen.getByTestId("start-node-images");
    const chips = within(strip).getAllByTestId("start-node-image-chip");
    expect(chips).toHaveLength(2);
    expect(strip.textContent).toContain("ui-bug.png");
    expect(strip.textContent).toContain("trace.png");
  });

  it("renders no image strip when the run has no input images", () => {
    render(<EditNode {...markerProps({ nodeType: "start", inputImages: [] })} />, {
      wrapper: Wrapper,
    });
    expect(screen.queryByTestId("start-node-images")).toBeNull();
  });

  it("renders no image strip on a non-start node even if images are passed", () => {
    render(
      <EditNode {...markerProps({ nodeType: "end", inputImages: ["ui-bug.png"] })} />,
      { wrapper: Wrapper },
    );
    expect(screen.queryByTestId("start-node-images")).toBeNull();
  });
});

describe("EditNode rim drag-source (#844)", () => {
  function workProps(outputs: { name: string; side: "left" | "right" | "top" | "bottom" }[]) {
    return {
      ...markerProps({ nodeType: "agent" }),
      id: "nd_4f2a",
      data: {
        label: "rewrite_section",
        nodeId: "nd_4f2a",
        nodeType: "agent" as NodeType,
        status: "pending" as NodeStatus,
        reached: false,
        inputs: [],
        outputs,
        interactive: false,
        isolated: false,
      },
    } as unknown as Parameters<typeof EditNode>[0];
  }

  it("renders NO output dot — the card names no port any more", () => {
    const { container } = render(
      <EditNode {...workProps([{ name: "out", side: "right" }, { name: "spec", side: "right" }])} />,
      { wrapper: Wrapper },
    );
    expect(container.querySelectorAll(".port-dot")).toHaveLength(0);
    expect(container.querySelectorAll(".port-pill.kind-output")).toHaveLength(0);
    expect(screen.queryByText("out")).toBeNull();
    expect(screen.queryByText("spec")).toBeNull();
  });

  it("makes the whole border a connection source: one source strip per side", () => {
    const { container } = render(<EditNode {...workProps([{ name: "out", side: "right" }])} />, {
      wrapper: Wrapper,
    });
    for (const side of ["top", "bottom", "left", "right"]) {
      const strip = screen.getByTestId(`rim-source-${side}`);
      expect(strip.getAttribute("data-handleid")).toBe(`rim-${side}`);
    }
    expect(container.querySelectorAll('[data-testid^="rim-source-"]')).toHaveLength(4);
    // xyflow's own `.react-flow__handle` rule sets width/height: 6px, which beats
    // plain opposing insets — so each strip must say `auto` on its long axis or it
    // collapses to a 6px square in a corner, and every wire then leaves the card a
    // dozen pixels off the side's middle.
    const top = screen.getByTestId("rim-source-top") as HTMLElement;
    expect(top.style.width).toBe("auto");
    const left = screen.getByTestId("rim-source-left") as HTMLElement;
    expect(left.style.height).toBe("auto");
    // Above the `inset: 0` body target handle, so the rim wins the pointer.
    expect(Number(top.style.zIndex)).toBeGreaterThan(1);
  });

  it("grows no rim on a node that declares no output — it starts no wire", () => {
    render(<EditNode {...workProps([])} />, { wrapper: Wrapper });
    expect(screen.queryByTestId("rim-source-top")).toBeNull();
  });

  it("arms the gesture with an amber ring while the rim is hovered", () => {
    const { container } = render(<EditNode {...workProps([{ name: "out", side: "right" }])} />, {
      wrapper: Wrapper,
    });
    const card = container.firstElementChild as HTMLElement;
    expect(card.style.boxShadow).toBe("");
    fireEvent.pointerEnter(screen.getByTestId("rim-source-bottom"));
    expect(card.style.boxShadow).toContain("--color-st-await");
    fireEvent.pointerLeave(screen.getByTestId("rim-source-bottom"));
    expect(card.style.boxShadow).toBe("");
  });
});

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
      const gesture = startGesture({ x: 80, y: 80 }, "bottom", GRID_STEP);
      const traced = droppedPath(gesture, AIM[side], END, BODY, "src", GRID_STEP);
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
