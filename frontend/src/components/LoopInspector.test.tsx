import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, beforeEach } from "vitest";
import LoopInspector from "./LoopInspector";
import { useEditStore } from "../stores/editStore";
import type { PipelineDef, NodeDef } from "../types";

function makeLoopNode(overrides?: Partial<NodeDef>): NodeDef {
  return {
    id: "loop1",
    name: "review-loop",
    type: "loop",
    inputs: [
      { name: "in", repeated: false, side: "left" },
      { name: "break", repeated: false, side: "left" },
    ],
    outputs: [
      { name: "body", repeated: false, side: "right" },
      { name: "done", repeated: false, side: "right" },
    ],
    interactive: false,
    max_iter: 5,
    ...overrides,
  };
}

function makePipeline(node: NodeDef): PipelineDef {
  return {
    name: "test-pipeline",
    variables: {},
    nodes: [node],
    edges: [],
  };
}

function setStoreState(node: NodeDef) {
  const pipeline = makePipeline(node);
  useEditStore.setState({
    openTabs: [
      {
        id: "tab1",
        scope: "repo",
        pipeline,
        prompts: {},
        dirty: false,
        externalDirty: false,
      },
    ],
    activeTabId: "tab1",
    selection: { kind: "node", id: node.id },
  });
}

describe("LoopInspector", () => {
  beforeEach(() => {
    useEditStore.setState({
      openTabs: [],
      activeTabId: null,
      selection: { kind: "none", id: null },
    });
  });

  it("renders nothing when no loop node is selected", () => {
    const { container } = render(<LoopInspector />);
    expect(container.innerHTML).toBe("");
  });

  it("renders inspector title", () => {
    setStoreState(makeLoopNode());
    render(<LoopInspector />);
    expect(screen.getByText("Loop Inspector")).toBeInTheDocument();
  });

  it("displays the node ID", () => {
    setStoreState(makeLoopNode());
    render(<LoopInspector />);
    expect(screen.getByText("loop1")).toBeInTheDocument();
  });

  it("displays max_iter input with current value", () => {
    setStoreState(makeLoopNode());
    render(<LoopInspector />);
    const input = screen.getByTestId("max-iter-input") as HTMLInputElement;
    expect(input.defaultValue).toBe("5");
  });

  it("renders all 4 fixed ports", () => {
    setStoreState(makeLoopNode());
    render(<LoopInspector />);
    expect(screen.getByTestId("port-row-in")).toBeInTheDocument();
    expect(screen.getByTestId("port-row-break")).toBeInTheDocument();
    expect(screen.getByTestId("port-row-body")).toBeInTheDocument();
    expect(screen.getByTestId("port-row-done")).toBeInTheDocument();
  });

  it("shows input/output badges on ports", () => {
    setStoreState(makeLoopNode());
    render(<LoopInspector />);
    const badges = screen.getAllByText(/^(input|output)$/);
    const inputBadges = badges.filter((b) => b.textContent === "input");
    const outputBadges = badges.filter((b) => b.textContent === "output");
    expect(inputBadges).toHaveLength(2);
    expect(outputBadges).toHaveLength(2);
  });

  it("shows port count of 4", () => {
    setStoreState(makeLoopNode());
    render(<LoopInspector />);
    expect(screen.getByText("4")).toBeInTheDocument();
  });

  it("shows help text about fixed ports", () => {
    setStoreState(makeLoopNode());
    render(<LoopInspector />);
    expect(screen.getByText(/Port names are fixed/)).toBeInTheDocument();
  });

  it("updates max_iter via blur", () => {
    setStoreState(makeLoopNode());
    render(<LoopInspector />);
    const input = screen.getByTestId("max-iter-input") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "10" } });
    fireEvent.blur(input);

    const state = useEditStore.getState();
    const updatedNode = state.openTabs[0].pipeline.nodes.find((n) => n.id === "loop1")!;
    expect(updatedNode.max_iter).toBe(10);
  });

  it("supports variable reference for max_iter", () => {
    setStoreState(makeLoopNode());
    render(<LoopInspector />);
    const input = screen.getByTestId("max-iter-input") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "$max_iter_review" } });
    fireEvent.blur(input);

    const state = useEditStore.getState();
    const updatedNode = state.openTabs[0].pipeline.nodes.find((n) => n.id === "loop1")!;
    expect(updatedNode.max_iter).toBe("$max_iter_review");
  });
});
