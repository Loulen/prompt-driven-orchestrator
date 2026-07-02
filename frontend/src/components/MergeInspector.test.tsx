import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, beforeEach } from "vitest";
import MergeInspector from "./MergeInspector";
import { useEditStore } from "../stores/editStore";
import type { PipelineDef, NodeDef } from "../types";

function makeMergeNode(overrides?: Partial<NodeDef>): NodeDef {
  return {
    id: "mg1",
    name: "merge-point",
    type: "merge",
    inputs: [{ name: "branches", repeated: true, side: "left" }],
    outputs: [{ name: "merged", repeated: false, side: "right" }],
    interactive: false,
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
        diagnostics: [],
        dirty: false,
        externalDirty: false,
      },
    ],
    activeTabId: "tab1",
    selection: { kind: "node", id: node.id },
  });
}

describe("MergeInspector", () => {
  beforeEach(() => {
    useEditStore.setState({
      openTabs: [],
      activeTabId: null,
      selection: { kind: "none", id: null },
    });
  });

  it("renders nothing when no tab is active", () => {
    const { container } = render(<MergeInspector />);
    expect(container.firstChild).toBeNull();
  });

  it("renders the inspector header", () => {
    setStoreState(makeMergeNode());
    render(<MergeInspector />);
    expect(screen.getByText("Merge Inspector")).toBeInTheDocument();
  });

  it("displays the node ID", () => {
    setStoreState(makeMergeNode());
    render(<MergeInspector />);
    expect(screen.getByText("mg1")).toBeInTheDocument();
  });

  it("displays the node name in an editable field", () => {
    setStoreState(makeMergeNode());
    render(<MergeInspector />);
    const nameInput = screen.getByDisplayValue("merge-point");
    expect(nameInput).toBeInTheDocument();
  });

  it("displays port labels", () => {
    setStoreState(makeMergeNode());
    render(<MergeInspector />);
    expect(screen.getByText("branches (repeated)")).toBeInTheDocument();
    expect(screen.getByText("merged")).toBeInTheDocument();
  });

  it("updates the name when changed", () => {
    setStoreState(makeMergeNode());
    render(<MergeInspector />);
    const nameInput = screen.getByDisplayValue("merge-point");
    fireEvent.change(nameInput, { target: { value: "new-merge" } });
    const tab = useEditStore.getState().openTabs[0];
    const node = tab.pipeline.nodes.find((n) => n.id === "mg1");
    expect(node?.name).toBe("new-merge");
  });

  it("displays the merge behavior description", () => {
    setStoreState(makeMergeNode());
    render(<MergeInspector />);
    expect(screen.getByText(/Merge nodes wait for all upstream/)).toBeInTheDocument();
  });

  // #296: a merge node spawns an agent, so its model is settable here too.
  it("writes the typed model onto the merge node", () => {
    setStoreState(makeMergeNode());
    render(<MergeInspector />);
    const input = screen.getByTestId("merge-model-input");
    fireEvent.change(input, { target: { value: "opus" } });
    const node = useEditStore.getState().openTabs[0].pipeline.nodes.find((n) => n.id === "mg1");
    expect(node?.model).toBe("opus");
  });

  it("renders a seeded model as the input value", () => {
    setStoreState(makeMergeNode({ model: "sonnet" }));
    render(<MergeInspector />);
    expect((screen.getByTestId("merge-model-input") as HTMLInputElement).value).toBe("sonnet");
  });
});
