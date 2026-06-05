import { render, screen, within } from "@testing-library/react";
import { describe, it, expect, afterEach } from "vitest";
import { ReactFlowProvider } from "@xyflow/react";
import { TooltipProvider } from "./ui/tooltip";
import { EditNode } from "./EditCanvas";
import { useEditStore } from "../stores/editStore";
import type { NodeStatus, NodeType } from "../types";

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
