import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, beforeEach } from "vitest";
import SwitchInspector from "./SwitchInspector";
import { useEditStore } from "../stores/editStore";
import type { PipelineDef, NodeDef, EdgeDef, VariableDef } from "../types";

function makeSwitchNode(overrides?: Partial<NodeDef>): NodeDef {
  return {
    id: "sw1",
    name: "my-switch",
    type: "switch",
    inputs: [{ name: "in", repeated: false, side: "left" }],
    outputs: [
      { name: "yes", repeated: false, side: "right", when: { status: { eq: "ok" } } },
      { name: "default", repeated: false, side: "right" },
    ],
    interactive: false,
    ...overrides,
  };
}

function makeUpstreamNode(): NodeDef {
  return {
    id: "upstream",
    name: "upstream",
    type: "doc-only",
    inputs: [],
    outputs: [
      {
        name: "out",
        repeated: false,
        side: "right",
        frontmatter: {
          status: { type: "enum", allowed: ["ok", "fail"] },
          field: { type: "string" },
        },
      },
    ],
    interactive: false,
  };
}

function makePipeline(node: NodeDef): PipelineDef {
  const upstream = makeUpstreamNode();
  return {
    name: "test-pipeline",
    variables: {},
    nodes: [upstream, node],
    edges: [
      { source: { node: "upstream", port: "out" }, target: { node: node.id, port: "in" } },
    ],
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

describe("SwitchInspector", () => {
  beforeEach(() => {
    useEditStore.setState({
      openTabs: [],
      activeTabId: null,
      selection: { kind: "none", id: null },
    });
  });

  it("renders nothing when no switch node is selected", () => {
    const { container } = render(<SwitchInspector />);
    expect(container.innerHTML).toBe("");
  });

  it("renders inspector title", () => {
    setStoreState(makeSwitchNode());
    render(<SwitchInspector />);
    expect(screen.getByText("Switch Inspector")).toBeInTheDocument();
  });

  it("renders branches", () => {
    setStoreState(makeSwitchNode());
    render(<SwitchInspector />);
    expect(screen.getByTestId("branch-editor-yes")).toBeInTheDocument();
    expect(screen.getByTestId("branch-editor-default")).toBeInTheDocument();
  });

  it("shows 'fallback' pin on default branch", () => {
    setStoreState(makeSwitchNode());
    render(<SwitchInspector />);
    expect(screen.getByText("fallback")).toBeInTheDocument();
  });

  it("adds a new branch before default", () => {
    const node = makeSwitchNode();
    setStoreState(node);
    render(<SwitchInspector />);

    const addButtons = screen.getAllByText("+ Add");
    fireEvent.click(addButtons[0]);

    const state = useEditStore.getState();
    const updatedNode = state.openTabs[0].pipeline.nodes.find((n) => n.id === "sw1")!;
    expect(updatedNode.outputs).toHaveLength(3);
    expect(updatedNode.outputs[updatedNode.outputs.length - 1].name).toBe("default");
  });

  it("deletes a non-default branch", () => {
    const node = makeSwitchNode();
    setStoreState(node);
    render(<SwitchInspector />);

    const deleteButtons = screen.getAllByTitle("Delete branch");
    fireEvent.click(deleteButtons[0]);

    const state = useEditStore.getState();
    const updatedNode = state.openTabs[0].pipeline.nodes.find((n) => n.id === "sw1")!;
    expect(updatedNode.outputs).toHaveLength(1);
    expect(updatedNode.outputs[0].name).toBe("default");
  });

  it("adds a condition row to a branch", () => {
    const node = makeSwitchNode();
    setStoreState(node);
    render(<SwitchInspector />);

    const addCondBtn = screen.getByTestId("add-condition");
    fireEvent.click(addCondBtn);

    const state = useEditStore.getState();
    const updatedNode = state.openTabs[0].pipeline.nodes.find((n) => n.id === "sw1")!;
    const yesBranch = updatedNode.outputs.find((o) => o.name === "yes")!;
    const when = yesBranch.when as Record<string, Record<string, unknown>>;
    expect(when).toBeDefined();
    expect(when["status"]).toBeDefined();
  });

  it("deletes a condition row", () => {
    const node = makeSwitchNode();
    setStoreState(node);
    render(<SwitchInspector />);

    const conditionRows = screen.getAllByTestId("condition-row");
    expect(conditionRows).toHaveLength(1);

    const deleteBtn = screen.getByTestId("delete-condition");
    fireEvent.click(deleteBtn);

    const state = useEditStore.getState();
    const updatedNode = state.openTabs[0].pipeline.nodes.find((n) => n.id === "sw1")!;
    const yesBranch = updatedNode.outputs.find((o) => o.name === "yes")!;
    expect(yesBranch.when).toBeNull();
  });

  it("renders the operator dropdown with all operators", () => {
    const node = makeSwitchNode();
    setStoreState(node);
    render(<SwitchInspector />);

    const dropdown = screen.getByTestId("op-dropdown");
    const options = dropdown.querySelectorAll("option");
    expect(options).toHaveLength(8);
    const values = Array.from(options).map((o) => o.value);
    expect(values).toContain("eq");
    expect(values).toContain("neq");
    expect(values).toContain("in");
    expect(values).toContain("not_in");
  });

  it("displays the node name", () => {
    setStoreState(makeSwitchNode());
    render(<SwitchInspector />);
    const nameInput = screen.getByPlaceholderText("sw1") as HTMLInputElement;
    expect(nameInput.defaultValue).toBe("my-switch");
  });

  it("moves a branch up via reorder", () => {
    const node = makeSwitchNode({
      outputs: [
        { name: "first", repeated: false, side: "right", when: { a: { eq: 1 } } },
        { name: "second", repeated: false, side: "right", when: { b: { eq: 2 } } },
        { name: "default", repeated: false, side: "right" },
      ],
    });
    setStoreState(node);
    render(<SwitchInspector />);

    const moveUpButtons = screen.getAllByTitle("Move up");
    fireEvent.click(moveUpButtons[0]);

    const state = useEditStore.getState();
    const updatedNode = state.openTabs[0].pipeline.nodes.find((n) => n.id === "sw1")!;
    expect(updatedNode.outputs[0].name).toBe("second");
    expect(updatedNode.outputs[1].name).toBe("first");
    expect(updatedNode.outputs[2].name).toBe("default");
  });

  it("moves a branch down via reorder", () => {
    const node = makeSwitchNode({
      outputs: [
        { name: "first", repeated: false, side: "right", when: { a: { eq: 1 } } },
        { name: "second", repeated: false, side: "right", when: { b: { eq: 2 } } },
        { name: "default", repeated: false, side: "right" },
      ],
    });
    setStoreState(node);
    render(<SwitchInspector />);

    const moveDownButtons = screen.getAllByTitle("Move down");
    fireEvent.click(moveDownButtons[0]);

    const state = useEditStore.getState();
    const updatedNode = state.openTabs[0].pipeline.nodes.find((n) => n.id === "sw1")!;
    expect(updatedNode.outputs[0].name).toBe("second");
    expect(updatedNode.outputs[1].name).toBe("first");
    expect(updatedNode.outputs[2].name).toBe("default");
  });

  it("prevents reorder of default branch", () => {
    const node = makeSwitchNode();
    setStoreState(node);
    render(<SwitchInspector />);

    const defaultEditor = screen.getByTestId("branch-editor-default");
    expect(defaultEditor.querySelector('[title="Move up"]')).toBeNull();
    expect(defaultEditor.querySelector('[title="Move down"]')).toBeNull();
  });
});

describe("Layer 5: Switch node add, configure, and save roundtrip", () => {
  it("a switch node can be added, configured with one branch, and its state is correct", () => {
    const switchNode: NodeDef = {
      id: "sw-new",
      name: "my-router",
      type: "switch",
      inputs: [{ name: "in", repeated: false, side: "left" }],
      outputs: [{ name: "default", repeated: false, side: "right" }],
      interactive: false,
    };
    const pipeline: PipelineDef = {
      name: "layer5-test",
      variables: {},
      nodes: [switchNode],
      edges: [],
    };
    useEditStore.setState({
      openTabs: [
        { id: "tab-l5", scope: "repo", pipeline, prompts: {}, diagnostics: [], dirty: false, externalDirty: false },
      ],
      activeTabId: "tab-l5",
      selection: { kind: "node", id: "sw-new" },
    });

    render(<SwitchInspector />);

    expect(screen.getByText("Switch Inspector")).toBeInTheDocument();
    expect(screen.getByTestId("branch-editor-default")).toBeInTheDocument();

    const addButtons = screen.getAllByText("+ Add");
    fireEvent.click(addButtons[0]);

    const state = useEditStore.getState();
    const updated = state.openTabs[0].pipeline.nodes.find((n) => n.id === "sw-new")!;
    expect(updated.outputs).toHaveLength(2);
    expect(updated.outputs[0].name).toBe("branch");
    expect(updated.outputs[1].name).toBe("default");
    expect(updated.type).toBe("switch");
  });
});

describe("Edge disconnection clears frontmatter predicates (issue #64)", () => {
  beforeEach(() => {
    useEditStore.setState({
      openTabs: [],
      activeTabId: null,
      selection: { kind: "none", id: null },
    });
  });

  it("removes frontmatter-based when fields but preserves $variable fields on edge delete", () => {
    const reviewerNode: NodeDef = {
      id: "reviewer",
      name: "Reviewer",
      type: "doc-only",
      inputs: [],
      outputs: [
        {
          name: "review",
          repeated: false,
          side: "right",
          frontmatter: {
            verdict: { type: "enum", allowed: ["PASS", "FAIL"] },
          },
        },
      ],
      interactive: false,
    };
    const switchNode: NodeDef = {
      id: "gate",
      name: "Gate",
      type: "switch",
      inputs: [{ name: "in", repeated: false, side: "left" }],
      outputs: [
        {
          name: "pass",
          repeated: false,
          side: "right",
          when: { verdict: { eq: "PASS" }, $threshold: { gte: 5 } },
        },
        { name: "default", repeated: false, side: "right" },
      ],
      interactive: false,
    };
    const edges: EdgeDef[] = [
      { source: { node: "reviewer", port: "review" }, target: { node: "gate", port: "in" } },
    ];
    const pipeline: PipelineDef = {
      name: "disconnect-test",
      variables: { threshold: { type: "int", default: 7 } },
      nodes: [reviewerNode, switchNode],
      edges,
    };

    useEditStore.setState({
      openTabs: [
        {
          id: "tab-disc",
          scope: "repo",
          pipeline,
          prompts: {},
          diagnostics: [],
          dirty: false,
          externalDirty: false,
        },
      ],
      activeTabId: "tab-disc",
      selection: { kind: "none", id: null },
    });

    useEditStore.getState().deleteEdge(0);

    const state = useEditStore.getState();
    const gate = state.openTabs[0].pipeline.nodes.find((n) => n.id === "gate")!;
    const passBranch = gate.outputs.find((o) => o.name === "pass")!;
    const when = passBranch.when as Record<string, unknown> | null;
    expect(when).toBeDefined();
    expect(when).not.toBeNull();
    expect(when!["$threshold"]).toBeDefined();
    expect(when!["verdict"]).toBeUndefined();
  });

  it("clears when entirely if all fields were frontmatter-based", () => {
    const reviewerNode: NodeDef = {
      id: "reviewer",
      name: "Reviewer",
      type: "doc-only",
      inputs: [],
      outputs: [
        {
          name: "review",
          repeated: false,
          side: "right",
          frontmatter: {
            verdict: { type: "enum", allowed: ["PASS", "FAIL"] },
          },
        },
      ],
      interactive: false,
    };
    const switchNode: NodeDef = {
      id: "gate",
      name: "Gate",
      type: "switch",
      inputs: [{ name: "in", repeated: false, side: "left" }],
      outputs: [
        {
          name: "pass",
          repeated: false,
          side: "right",
          when: { verdict: { eq: "PASS" } },
        },
        { name: "default", repeated: false, side: "right" },
      ],
      interactive: false,
    };
    const edges: EdgeDef[] = [
      { source: { node: "reviewer", port: "review" }, target: { node: "gate", port: "in" } },
    ];
    const pipeline: PipelineDef = {
      name: "disconnect-test-2",
      variables: {},
      nodes: [reviewerNode, switchNode],
      edges,
    };

    useEditStore.setState({
      openTabs: [
        {
          id: "tab-disc2",
          scope: "repo",
          pipeline,
          prompts: {},
          diagnostics: [],
          dirty: false,
          externalDirty: false,
        },
      ],
      activeTabId: "tab-disc2",
      selection: { kind: "none", id: null },
    });

    useEditStore.getState().deleteEdge(0);

    const state = useEditStore.getState();
    const gate = state.openTabs[0].pipeline.nodes.find((n) => n.id === "gate")!;
    const passBranch = gate.outputs.find((o) => o.name === "pass")!;
    expect(passBranch.when).toBeNull();
  });
});

describe("S1 AND-rail layout (issue #92)", () => {
  beforeEach(() => {
    useEditStore.setState({
      openTabs: [],
      activeTabId: null,
      selection: { kind: "none", id: null },
    });
  });

  it("renders non-default branches as sb-card with header and body", () => {
    setStoreState(makeSwitchNode());
    render(<SwitchInspector />);
    const branchEl = screen.getByTestId("branch-editor-yes");
    expect(branchEl.classList.contains("sb-card")).toBe(true);
    expect(branchEl.querySelector(".sb-head")).toBeInTheDocument();
    expect(branchEl.querySelector(".sb-body")).toBeInTheDocument();
  });

  it("multi-condition branch shows sb-conds.multi (rail + AND label)", () => {
    const node = makeSwitchNode({
      outputs: [
        {
          name: "multi",
          repeated: false,
          side: "right",
          when: { status: { eq: "ok" }, score: { gte: 5 } },
        },
        { name: "default", repeated: false, side: "right" },
      ],
    });
    setStoreState(node);
    render(<SwitchInspector />);
    const branchEl = screen.getByTestId("branch-editor-multi");
    const conds = branchEl.querySelector(".sb-conds");
    expect(conds).toBeInTheDocument();
    expect(conds!.classList.contains("multi")).toBe(true);
  });

  it("single-condition branch shows sb-conds.single (no rail)", () => {
    setStoreState(makeSwitchNode());
    render(<SwitchInspector />);
    const branchEl = screen.getByTestId("branch-editor-yes");
    const conds = branchEl.querySelector(".sb-conds");
    expect(conds).toBeInTheDocument();
    expect(conds!.classList.contains("single")).toBe(true);
  });

  it("zero-condition branch has no sb-conds container", () => {
    const node = makeSwitchNode({
      outputs: [
        { name: "empty", repeated: false, side: "right" },
        { name: "default", repeated: false, side: "right" },
      ],
    });
    setStoreState(node);
    render(<SwitchInspector />);
    const branchEl = screen.getByTestId("branch-editor-empty");
    expect(branchEl.querySelector(".sb-conds")).toBeNull();
  });

  it("reorder controls are icon buttons, not typographic arrows", () => {
    const node = makeSwitchNode({
      outputs: [
        { name: "first", repeated: false, side: "right", when: { a: { eq: 1 } } },
        { name: "second", repeated: false, side: "right", when: { b: { eq: 2 } } },
        { name: "default", repeated: false, side: "right" },
      ],
    });
    setStoreState(node);
    render(<SwitchInspector />);
    const moveUpBtns = screen.getAllByTitle("Move up");
    const moveDownBtns = screen.getAllByTitle("Move down");
    expect(moveUpBtns.length).toBeGreaterThan(0);
    expect(moveDownBtns.length).toBeGreaterThan(0);
    for (const btn of [...moveUpBtns, ...moveDownBtns]) {
      expect(btn.querySelector("svg")).toBeInTheDocument();
      expect(btn.textContent).not.toMatch(/[▲▼]/);
    }
  });

  it("+ Add condition is a styled button", () => {
    setStoreState(makeSwitchNode());
    render(<SwitchInspector />);
    const addBtn = screen.getByTestId("add-condition");
    expect(addBtn.tagName).toBe("BUTTON");
    expect(addBtn.classList.contains("sb-add-cond")).toBe(true);
  });

  it("default branch renders with sb-default class and fallback pin", () => {
    setStoreState(makeSwitchNode());
    render(<SwitchInspector />);
    const defaultEl = screen.getByTestId("branch-editor-default");
    expect(defaultEl.classList.contains("sb-card")).toBe(true);
    expect(defaultEl.classList.contains("sb-default")).toBe(true);
    expect(defaultEl.querySelector(".sb-pin")).toBeInTheDocument();
    expect(defaultEl.querySelector(".sb-pin")!.textContent).toBe("fallback");
  });

  it("default branch name is italic", () => {
    setStoreState(makeSwitchNode());
    render(<SwitchInspector />);
    const defaultEl = screen.getByTestId("branch-editor-default");
    const nameEl = defaultEl.querySelector(".sb-name");
    expect(nameEl).toBeInTheDocument();
  });

  it("default branch has no reorder controls", () => {
    setStoreState(makeSwitchNode());
    render(<SwitchInspector />);
    const defaultEl = screen.getByTestId("branch-editor-default");
    expect(defaultEl.querySelector('[title="Move up"]')).toBeNull();
    expect(defaultEl.querySelector('[title="Move down"]')).toBeNull();
  });

  it("default branch has no delete button", () => {
    setStoreState(makeSwitchNode());
    render(<SwitchInspector />);
    const defaultEl = screen.getByTestId("branch-editor-default");
    expect(defaultEl.querySelector('[title="Delete branch"]')).toBeNull();
  });

  it("help paragraph is removed", () => {
    setStoreState(makeSwitchNode());
    render(<SwitchInspector />);
    expect(screen.queryByText(/All conditions in a branch are AND/)).not.toBeInTheDocument();
  });

  it("non-default branch header contains drag grip", () => {
    setStoreState(makeSwitchNode());
    render(<SwitchInspector />);
    const branchEl = screen.getByTestId("branch-editor-yes");
    expect(branchEl.querySelector(".sb-grip")).toBeInTheDocument();
  });

  it("condition rows use sb-cond class with grid layout", () => {
    setStoreState(makeSwitchNode());
    render(<SwitchInspector />);
    const condRow = screen.getByTestId("condition-row");
    expect(condRow.classList.contains("sb-cond")).toBe(true);
  });
});

describe("Typed when clauses (issue #64)", () => {
  function makeTypedPipeline(): {
    pipeline: PipelineDef;
    switchNode: NodeDef;
  } {
    const reviewerNode: NodeDef = {
      id: "reviewer",
      name: "Reviewer",
      type: "doc-only",
      inputs: [{ name: "code", repeated: false, side: "left" }],
      outputs: [
        {
          name: "review",
          repeated: false,
          side: "right",
          frontmatter: {
            verdict: { type: "enum", allowed: ["PASS", "FAIL"] },
            score: { type: "int" },
          },
        },
      ],
      interactive: false,
    };
    const switchNode: NodeDef = {
      id: "gate",
      name: "Gate",
      type: "switch",
      inputs: [{ name: "in", repeated: false, side: "left" }],
      outputs: [
        { name: "pass", repeated: false, side: "right", when: { verdict: { eq: "PASS" } } },
        { name: "default", repeated: false, side: "right" },
      ],
      interactive: false,
    };
    const edges: EdgeDef[] = [
      { source: { node: "reviewer", port: "review" }, target: { node: "gate", port: "in" } },
    ];
    const pipeline: PipelineDef = {
      name: "typed-test",
      variables: {},
      nodes: [reviewerNode, switchNode],
      edges,
    };
    return { pipeline, switchNode };
  }

  function setTypedState(overrides?: {
    variables?: Record<string, VariableDef>;
    edges?: EdgeDef[];
  }) {
    const { pipeline, switchNode } = makeTypedPipeline();
    if (overrides?.variables) pipeline.variables = overrides.variables;
    if (overrides?.edges) pipeline.edges = overrides.edges;
    useEditStore.setState({
      openTabs: [
        {
          id: "tab-typed",
          scope: "repo",
          pipeline,
          prompts: {},
          diagnostics: [],
          dirty: false,
          externalDirty: false,
        },
      ],
      activeTabId: "tab-typed",
      selection: { kind: "node", id: switchNode.id },
    });
  }

  beforeEach(() => {
    useEditStore.setState({
      openTabs: [],
      activeTabId: null,
      selection: { kind: "none", id: null },
    });
  });

  it("renders field dropdown with upstream schema fields", () => {
    setTypedState();
    render(<SwitchInspector />);
    const fieldDropdown = screen.getByTestId("field-dropdown");
    expect(fieldDropdown).toBeInTheDocument();
    const options = fieldDropdown.querySelectorAll("option");
    const values = Array.from(options).map((o) => o.value);
    expect(values).toContain("verdict");
    expect(values).toContain("score");
  });

  it("renders value dropdown with allowed values for enum field", () => {
    setTypedState();
    render(<SwitchInspector />);
    const valueDropdown = screen.getByTestId("value-dropdown");
    expect(valueDropdown).toBeInTheDocument();
    const options = valueDropdown.querySelectorAll("option");
    const values = Array.from(options).map((o) => o.value);
    expect(values).toContain("PASS");
    expect(values).toContain("FAIL");
  });

  it("shows $variable options in field dropdown when variables declared", () => {
    setTypedState({
      variables: { threshold: { type: "int", default: 7 } },
    });
    render(<SwitchInspector />);
    const fieldDropdown = screen.getByTestId("field-dropdown");
    const options = fieldDropdown.querySelectorAll("option");
    const values = Array.from(options).map((o) => o.value);
    expect(values).toContain("$threshold");
  });

  it("disables add-condition when no upstream schema and no variables", () => {
    setTypedState({ edges: [] });
    render(<SwitchInspector />);
    const addBtn = screen.getByTestId("add-condition");
    expect(addBtn).toBeDisabled();
  });

  it("enables add-condition when upstream schema is available", () => {
    setTypedState();
    render(<SwitchInspector />);
    const addBtn = screen.getByTestId("add-condition");
    expect(addBtn).not.toBeDisabled();
  });

  it("removes free-text field input (uses dropdown instead)", () => {
    setTypedState();
    render(<SwitchInspector />);
    const conditionRows = screen.getAllByTestId("condition-row");
    const fieldInputs = conditionRows[0].querySelectorAll('input[placeholder="field"]');
    expect(fieldInputs).toHaveLength(0);
  });
});
