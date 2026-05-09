import { render, screen } from "@testing-library/react";
import { describe, it, expect, beforeEach } from "vitest";
import { ReactFlowProvider } from "@xyflow/react";
import { TooltipProvider } from "./ui/tooltip";
import { MergeEditNode, MergeRunNode } from "./MergeNode";
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

const baseMergeEditData = {
  label: "merge-point",
  nodeId: "mg1",
  inputSide: "left" as const,
  outputSide: "right" as const,
};

const baseMergeRunData = {
  label: "merge-point",
  nodeId: "mg1",
  status: "completed" as NodeStatus,
  iter: 1,
  inputSide: "left" as const,
  outputSide: "right" as const,
};

function editProps(
  overrides?: Partial<typeof baseMergeEditData>,
): NodeProps<Node<typeof baseMergeEditData>> {
  const data = { ...baseMergeEditData, ...overrides };
  return {
    id: "mg1",
    data,
    type: "merge",
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
  } as unknown as NodeProps<Node<typeof baseMergeEditData>>;
}

function runProps(
  overrides?: Partial<typeof baseMergeRunData>,
): NodeProps<Node<typeof baseMergeRunData>> {
  const data = { ...baseMergeRunData, ...overrides };
  return {
    id: "mg1",
    data,
    type: "mergeRun",
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
  } as unknown as NodeProps<Node<typeof baseMergeRunData>>;
}

describe("MergeEditNode", () => {
  beforeEach(() => {
    useEditStore.setState({
      selection: { kind: "none", id: null },
    });
  });

  it("renders the node label", () => {
    render(<MergeEditNode {...editProps()} />, { wrapper: Wrapper });
    expect(screen.getByText("merge-point")).toBeInTheDocument();
  });

  it("renders 'merge' type badge", () => {
    render(<MergeEditNode {...editProps()} />, { wrapper: Wrapper });
    expect(screen.getByText("merge")).toBeInTheDocument();
  });

  it("renders the node id", () => {
    render(<MergeEditNode {...editProps()} />, { wrapper: Wrapper });
    expect(screen.getByText("mg1")).toBeInTheDocument();
  });

  it("renders labeled rows for ports", () => {
    render(<MergeEditNode {...editProps()} />, { wrapper: Wrapper });
    expect(screen.getByTestId("port-input-branches")).toBeInTheDocument();
    expect(screen.getByTestId("port-output-merged")).toBeInTheDocument();
  });

  it("shows selected ring when selected", () => {
    useEditStore.setState({
      selection: { kind: "node", id: "mg1" },
    });
    const { container } = render(<MergeEditNode {...editProps()} />, {
      wrapper: Wrapper,
    });
    expect(container.firstElementChild?.className).toContain("ring-1");
  });
});

describe("MergeRunNode", () => {
  it("renders the node label", () => {
    render(<MergeRunNode {...runProps()} />, { wrapper: Wrapper });
    expect(screen.getByText("merge-point")).toBeInTheDocument();
  });

  it("renders 'merge' type badge", () => {
    render(<MergeRunNode {...runProps()} />, { wrapper: Wrapper });
    expect(screen.getByText("merge")).toBeInTheDocument();
  });

  it("renders labeled rows for ports", () => {
    render(<MergeRunNode {...runProps()} />, { wrapper: Wrapper });
    expect(screen.getByTestId("port-input-branches")).toBeInTheDocument();
    expect(screen.getByTestId("port-output-merged")).toBeInTheDocument();
  });

  it("shows status text", () => {
    render(<MergeRunNode {...runProps({ status: "running" })} />, {
      wrapper: Wrapper,
    });
    expect(screen.getByText("running")).toBeInTheDocument();
  });

  it("shows iteration badge when iter > 1", () => {
    render(<MergeRunNode {...runProps({ iter: 3 })} />, { wrapper: Wrapper });
    expect(screen.getByText("iter 3")).toBeInTheDocument();
  });

  it("does not show iteration badge when iter is 1", () => {
    render(<MergeRunNode {...runProps({ iter: 1 })} />, { wrapper: Wrapper });
    expect(screen.queryByText(/iter/)).not.toBeInTheDocument();
  });
});
