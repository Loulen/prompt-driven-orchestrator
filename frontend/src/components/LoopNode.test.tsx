import { render, screen } from "@testing-library/react";
import { describe, it, expect, beforeEach } from "vitest";
import { ReactFlowProvider } from "@xyflow/react";
import { TooltipProvider } from "./ui/tooltip";
import { LoopEditNode, LoopRunNode } from "./LoopNode";
import { useEditStore } from "../stores/editStore";
import type { NodeProps, Node } from "@xyflow/react";
import type { NodeStatus } from "../types";

function Wrapper({ children }: { children: React.ReactNode }) {
  return (
    <TooltipProvider>
      <ReactFlowProvider>{children}</ReactFlowProvider>
    </TooltipProvider>
  );
}

const baseLoopEditData = {
  label: "review-loop",
  nodeId: "loop1",
  maxIter: 5,
  ports: [
    { name: "in", kind: "input" as const, side: "left" as const },
    { name: "break", kind: "input" as const, side: "left" as const },
    { name: "body", kind: "output" as const, side: "right" as const },
    { name: "done", kind: "output" as const, side: "right" as const },
  ],
};

const baseLoopRunData = {
  label: "review-loop",
  nodeId: "loop1",
  status: "running" as NodeStatus,
  maxIter: 5,
  currentIter: 2,
  ports: [
    { name: "in", kind: "input" as const, side: "left" as const },
    { name: "break", kind: "input" as const, side: "left" as const },
    { name: "body", kind: "output" as const, side: "right" as const },
    { name: "done", kind: "output" as const, side: "right" as const },
  ],
};

function editProps(
  overrides?: Partial<typeof baseLoopEditData>,
): NodeProps<Node<typeof baseLoopEditData>> {
  const data = { ...baseLoopEditData, ...overrides };
  return {
    id: "loop1",
    data,
    type: "loop",
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
    width: 200,
    height: 100,
  } as unknown as NodeProps<Node<typeof baseLoopEditData>>;
}

function runProps(
  overrides?: Partial<typeof baseLoopRunData>,
): NodeProps<Node<typeof baseLoopRunData>> {
  const data = { ...baseLoopRunData, ...overrides };
  return {
    id: "loop1",
    data,
    type: "loopRun",
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
    width: 200,
    height: 100,
  } as unknown as NodeProps<Node<typeof baseLoopRunData>>;
}

describe("LoopEditNode", () => {
  beforeEach(() => {
    useEditStore.setState({
      selection: { kind: "none", id: null },
    });
  });

  it("renders the node label", () => {
    render(<LoopEditNode {...editProps()} />, { wrapper: Wrapper });
    expect(screen.getByText("review-loop")).toBeInTheDocument();
  });

  it("renders 'loop' type badge", () => {
    render(<LoopEditNode {...editProps()} />, { wrapper: Wrapper });
    expect(screen.getByText("loop")).toBeInTheDocument();
  });

  it("shows edit-mode iter badge with max N format", () => {
    render(<LoopEditNode {...editProps()} />, { wrapper: Wrapper });
    const badge = screen.getByTestId("iter-badge");
    expect(badge.textContent).toContain("max 5");
  });

  it("renders labeled rows for all ports", () => {
    render(<LoopEditNode {...editProps()} />, { wrapper: Wrapper });
    expect(screen.getByTestId("port-input-in")).toBeInTheDocument();
    expect(screen.getByTestId("port-input-break")).toBeInTheDocument();
    expect(screen.getByTestId("port-output-body")).toBeInTheDocument();
    expect(screen.getByTestId("port-output-done")).toBeInTheDocument();
  });

  it("shows selected ring when selected", () => {
    useEditStore.setState({
      selection: { kind: "node", id: "loop1" },
    });
    const { container } = render(<LoopEditNode {...editProps()} />, {
      wrapper: Wrapper,
    });
    expect(container.firstElementChild?.className).toContain("ring-1");
  });
});

describe("LoopRunNode", () => {
  it("renders the node label", () => {
    render(<LoopRunNode {...runProps()} />, { wrapper: Wrapper });
    expect(screen.getByText("review-loop")).toBeInTheDocument();
  });

  it("shows run-mode iter badge with k/N format", () => {
    render(<LoopRunNode {...runProps()} />, { wrapper: Wrapper });
    const badge = screen.getByTestId("iter-badge");
    expect(badge.textContent).toContain("2/5");
  });

  it("renders labeled rows for all ports", () => {
    render(<LoopRunNode {...runProps()} />, { wrapper: Wrapper });
    expect(screen.getByTestId("port-input-in")).toBeInTheDocument();
    expect(screen.getByTestId("port-input-break")).toBeInTheDocument();
    expect(screen.getByTestId("port-output-body")).toBeInTheDocument();
    expect(screen.getByTestId("port-output-done")).toBeInTheDocument();
  });

  it("shows status text", () => {
    render(<LoopRunNode {...runProps({ status: "completed" })} />, {
      wrapper: Wrapper,
    });
    expect(screen.getByText("completed")).toBeInTheDocument();
  });

  it("shows 'loop' type badge", () => {
    render(<LoopRunNode {...runProps()} />, { wrapper: Wrapper });
    expect(screen.getByText("loop")).toBeInTheDocument();
  });

  it("iter badge updates with different values", () => {
    render(<LoopRunNode {...runProps({ currentIter: 3, maxIter: 10 })} />, {
      wrapper: Wrapper,
    });
    const badge = screen.getByTestId("iter-badge");
    expect(badge.textContent).toContain("3/10");
  });
});
