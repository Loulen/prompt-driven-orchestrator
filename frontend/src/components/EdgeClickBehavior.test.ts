import { describe, it, expect, beforeEach, vi } from "vitest";
import { useEditStore } from "../pipeline/store/editStore";
import { edgeIndexFromId } from "./editNodeDerivation";
import type { EdgeDef } from "../shared/generated/EdgeDef";
import type { EditorNodeDef, EditorPipelineDef } from "../pipeline/model/pipeline";

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
// The pipeline context's API answers with the mock of `../api` above.
vi.mock("../pipeline/api/pipelines", async (importOriginal) => ({
  ...(await importOriginal<object>()),
  ...(await import("../api")),
}));

function makePipeline(nodes: EditorNodeDef[] = [], edges: EdgeDef[] = []): EditorPipelineDef {
  return { name: "test", version: "1.0", variables: {}, nodes, edges };
}

function makeNode(overrides: Partial<EditorNodeDef> = {}): EditorNodeDef {
  return {
    id: "default",
    name: "Default",
    type: "agent",
    inputs: [{ name: "in", repeated: false }],
    outputs: [{ name: "out", repeated: false }],
    interactive: false,
    view: { x: 100, y: 100 },
    ...overrides,
  };
}

function seedTabWithPipeline(pipeline: EditorPipelineDef) {
  useEditStore.setState({
    openTabs: [
      {
        id: "test-tab",
        scope: "repo",
        pipeline,
        prompts: {},
        diagnostics: [],
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

// Edge click opens the edge detail panel (#147): it selects the EDGE itself,
// keyed by its index in `pipeline.edges`, rather than redirecting to the source
// node. The xyflow edge id is `e-{index}`; `edgeIndexFromId` decodes it.
describe("edge click selects the edge", () => {
  it("decodes the edge index from the canvas edge id", () => {
    expect(edgeIndexFromId("e-0")).toBe(0);
    expect(edgeIndexFromId("e-3")).toBe(3);
  });

  it("returns null for an id that is not an edge id", () => {
    expect(edgeIndexFromId("node-abc")).toBeNull();
    expect(edgeIndexFromId("e-")).toBeNull();
    expect(edgeIndexFromId("")).toBeNull();
  });

  it("sets an edge selection carrying the clicked edge's index", () => {
    const nodeA = makeNode({ id: "aaaaaaaa", outputs: [{ name: "result", repeated: false }] });
    const nodeB = makeNode({ id: "bbbbbbbb" });
    const edge: EdgeDef = {
      source: { node: "aaaaaaaa", port: "result" },
      target: { node: "bbbbbbbb", port: "in" },
    };
    seedTabWithPipeline(makePipeline([nodeA, nodeB], [edge]));

    const idx = edgeIndexFromId("e-0");
    useEditStore.getState().setSelection({ kind: "edge", id: null, edgeIndex: idx! });

    const sel = useEditStore.getState().selection;
    expect(sel.kind).toBe("edge");
    expect(sel.edgeIndex).toBe(0);
  });
});

describe("SelectionKind includes edge", () => {
  it("setting selection to edge kind works", () => {
    useEditStore.getState().setSelection({ kind: "edge", id: null, edgeIndex: 1 });
    expect(useEditStore.getState().selection.kind).toBe("edge");
  });

  it("setting selection to node kind still works", () => {
    useEditStore.getState().setSelection({ kind: "node", id: "abc" });
    expect(useEditStore.getState().selection.kind).toBe("node");
  });

  it("setting selection to none kind works", () => {
    useEditStore.getState().setSelection({ kind: "none", id: null });
    expect(useEditStore.getState().selection.kind).toBe("none");
  });
});
