import { describe, it, expect, beforeEach, vi } from "vitest";
import { useEditStore } from "../stores/editStore";
import type { PipelineDef, NodeDef, EdgeDef } from "../types";

vi.mock("../api", () => ({
  fetchPipelines: vi.fn().mockResolvedValue([]),
  fetchPipeline: vi.fn().mockResolvedValue({
    scope: "repo",
    pipeline: { name: "test", version: "1.0", variables: {}, nodes: [], edges: [] },
    prompts: {},
  }),
  fetchRunPipeline: vi.fn().mockResolvedValue({
    scope: "run",
    pipeline: { name: "test", version: "1.0", variables: {}, nodes: [], edges: [] },
    prompts: {},
  }),
  savePipeline: vi.fn().mockResolvedValue(undefined),
  saveRunPipeline: vi.fn().mockResolvedValue(undefined),
}));

function makePipeline(nodes: NodeDef[] = [], edges: EdgeDef[] = []): PipelineDef {
  return { name: "test", version: "1.0", variables: {}, nodes, edges };
}

function makeNode(overrides: Partial<NodeDef> = {}): NodeDef {
  return {
    id: "default",
    name: "Default",
    type: "doc-only",
    inputs: [{ name: "in", repeated: false }],
    outputs: [{ name: "out", repeated: false }],
    interactive: false,
    view: { x: 100, y: 100 },
    ...overrides,
  };
}

function seedTabWithPipeline(pipeline: PipelineDef) {
  useEditStore.setState({
    openTabs: [
      {
        id: "test-tab",
        scope: "repo",
        pipeline,
        prompts: {},
        dirty: false,
        externalDirty: false,
      },
    ],
    activeTabId: "test-tab",
    selection: { kind: "none", id: null },
  });
}

beforeEach(() => {
  useEditStore.setState({
    pipelines: [],
    openTabs: [],
    activeTabId: null,
    selection: { kind: "none", id: null },
    scrollToPort: null,
    lastSavedAt: {},
  });
});

describe("edge click selects source node", () => {
  it("resolves edge source to node selection and sets scrollToPort", () => {
    const nodeA = makeNode({ id: "aaaaaaaa", name: "Alpha", outputs: [{ name: "result", repeated: false }] });
    const nodeB = makeNode({ id: "bbbbbbbb", name: "Beta" });
    const edge: EdgeDef = {
      source: { node: "aaaaaaaa", port: "result" },
      target: { node: "bbbbbbbb", port: "in" },
    };
    seedTabWithPipeline(makePipeline([nodeA, nodeB], [edge]));

    const tab = useEditStore.getState().openTabs[0];
    const edgeDef = tab.pipeline.edges[0];

    useEditStore.getState().setSelection({ kind: "node", id: edgeDef.source.node });
    useEditStore.getState().setScrollToPort(edgeDef.source.port);

    expect(useEditStore.getState().selection).toEqual({ kind: "node", id: "aaaaaaaa" });
    expect(useEditStore.getState().scrollToPort).toBe("result");
  });

  it("works for edges with default port names", () => {
    const nodeA = makeNode({ id: "aaaaaaaa" });
    const nodeB = makeNode({ id: "bbbbbbbb" });
    const edge: EdgeDef = {
      source: { node: "aaaaaaaa", port: "out" },
      target: { node: "bbbbbbbb", port: "in" },
    };
    seedTabWithPipeline(makePipeline([nodeA, nodeB], [edge]));

    const tab = useEditStore.getState().openTabs[0];
    const edgeDef = tab.pipeline.edges[0];

    useEditStore.getState().setSelection({ kind: "node", id: edgeDef.source.node });
    useEditStore.getState().setScrollToPort(edgeDef.source.port);

    expect(useEditStore.getState().selection).toEqual({ kind: "node", id: "aaaaaaaa" });
    expect(useEditStore.getState().scrollToPort).toBe("out");
  });
});

describe("scrollToPort store field", () => {
  it("defaults to null", () => {
    expect(useEditStore.getState().scrollToPort).toBeNull();
  });

  it("can be set and cleared", () => {
    useEditStore.getState().setScrollToPort("review");
    expect(useEditStore.getState().scrollToPort).toBe("review");

    useEditStore.getState().setScrollToPort(null);
    expect(useEditStore.getState().scrollToPort).toBeNull();
  });
});

describe("SelectionKind does not include edge", () => {
  it("setting selection to node kind works", () => {
    useEditStore.getState().setSelection({ kind: "node", id: "abc" });
    expect(useEditStore.getState().selection.kind).toBe("node");
  });

  it("setting selection to none kind works", () => {
    useEditStore.getState().setSelection({ kind: "none", id: null });
    expect(useEditStore.getState().selection.kind).toBe("none");
  });
});
