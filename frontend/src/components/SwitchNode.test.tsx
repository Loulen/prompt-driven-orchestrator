import { render, screen } from "@testing-library/react";
import { describe, it, expect, beforeEach } from "vitest";
import { ReactFlowProvider } from "@xyflow/react";
import { TooltipProvider } from "./ui/tooltip";
import { SwitchEditNode, SwitchRunNode } from "./SwitchNode";
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

const baseSwitchEditData = {
  label: "route-requests",
  nodeId: "sw1",
  branches: [
    { name: "fast", side: "right" as const, hasWhen: true },
    { name: "default", side: "right" as const, hasWhen: false },
  ],
  inputSide: "left" as const,
};

const baseSwitchRunData = {
  label: "route-requests",
  nodeId: "sw1",
  status: "completed" as NodeStatus,
  branches: [
    { name: "fast", side: "right" as const, hasWhen: true },
    { name: "default", side: "right" as const, hasWhen: false },
  ],
  inputSide: "left" as const,
  activeBranch: "fast",
  iter: 1,
};

function editProps(
  overrides?: Partial<typeof baseSwitchEditData>,
): NodeProps<Node<typeof baseSwitchEditData>> {
  const data = { ...baseSwitchEditData, ...overrides };
  return {
    id: "sw1",
    data,
    type: "switch",
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
  } as unknown as NodeProps<Node<typeof baseSwitchEditData>>;
}

function runProps(
  overrides?: Partial<typeof baseSwitchRunData>,
): NodeProps<Node<typeof baseSwitchRunData>> {
  const data = { ...baseSwitchRunData, ...overrides };
  return {
    id: "sw1",
    data,
    type: "switchRun",
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
  } as unknown as NodeProps<Node<typeof baseSwitchRunData>>;
}

describe("SwitchEditNode", () => {
  beforeEach(() => {
    useEditStore.setState({
      selection: { kind: "none", id: null },
    });
  });

  it("renders the node label", () => {
    render(<SwitchEditNode {...editProps()} />, { wrapper: Wrapper });
    expect(screen.getByText("route-requests")).toBeInTheDocument();
  });

  it("renders 'switch' type badge", () => {
    render(<SwitchEditNode {...editProps()} />, { wrapper: Wrapper });
    expect(screen.getByText("switch")).toBeInTheDocument();
  });

  it("renders branch names as port labels", () => {
    render(<SwitchEditNode {...editProps()} />, { wrapper: Wrapper });
    expect(screen.getByText("fast")).toBeInTheDocument();
    expect(screen.getByText("default")).toBeInTheDocument();
  });

  it("renders labeled row for input port 'in'", () => {
    render(<SwitchEditNode {...editProps()} />, { wrapper: Wrapper });
    expect(screen.getByTestId("port-input-in")).toBeInTheDocument();
    expect(screen.getByTestId("port-input-in")).toHaveTextContent("in");
  });

  it("shows 'else' badge on default branch", () => {
    render(<SwitchEditNode {...editProps()} />, { wrapper: Wrapper });
    expect(screen.getByText("else")).toBeInTheDocument();
  });

  it("shows selected ring when selected", () => {
    useEditStore.setState({
      selection: { kind: "node", id: "sw1" },
    });
    const { container } = render(<SwitchEditNode {...editProps()} />, {
      wrapper: Wrapper,
    });
    expect(container.firstElementChild?.className).toContain("ring-1");
  });
});

describe("SwitchRunNode", () => {
  it("renders the node label", () => {
    render(<SwitchRunNode {...runProps()} />, { wrapper: Wrapper });
    expect(screen.getByText("route-requests")).toBeInTheDocument();
  });

  it("renders labeled row for input port 'in'", () => {
    render(<SwitchRunNode {...runProps()} />, { wrapper: Wrapper });
    expect(screen.getByTestId("port-input-in")).toBeInTheDocument();
  });

  it("highlights the active branch", () => {
    render(<SwitchRunNode {...runProps()} />, { wrapper: Wrapper });
    const fastBranch = screen.getByTestId("branch-fast");
    expect(fastBranch.className).toContain("bg-acc-bg");
  });

  it("dims non-active branches", () => {
    render(<SwitchRunNode {...runProps()} />, { wrapper: Wrapper });
    const defaultBranch = screen.getByTestId("branch-default");
    expect(defaultBranch.className).toContain("opacity-40");
  });

  it("shows iteration badge when iter > 1", () => {
    render(<SwitchRunNode {...runProps({ iter: 3 })} />, { wrapper: Wrapper });
    expect(screen.getByText("iter 3")).toBeInTheDocument();
  });

  it("does not show iteration badge when iter is 1", () => {
    render(<SwitchRunNode {...runProps({ iter: 1 })} />, { wrapper: Wrapper });
    expect(screen.queryByText(/iter/)).not.toBeInTheDocument();
  });

  it("renders 'switch' type badge", () => {
    render(<SwitchRunNode {...runProps()} />, { wrapper: Wrapper });
    expect(screen.getByText("switch")).toBeInTheDocument();
  });

  it("shows status text", () => {
    render(<SwitchRunNode {...runProps({ status: "running" })} />, {
      wrapper: Wrapper,
    });
    expect(screen.getByText("running")).toBeInTheDocument();
  });
});
