import { describe, it, expect, beforeEach, vi } from "vitest";
import { useEditStore } from "./editStore";
import { savePipeline, fetchPipeline } from "../api";
import type { PipelineDef, NodeDef, EdgeDef } from "../types";

vi.mock("../api", () => ({
  fetchPipelines: vi.fn().mockResolvedValue([]),
  fetchPipeline: vi.fn().mockResolvedValue({
    scope: "repo",
    pipeline: {
      name: "test",
      version: "1.0",
      variables: {},
      nodes: [],
      edges: [],
    },
    prompts: {},
    diagnostics: [],
  }),
  fetchRunPipeline: vi.fn().mockResolvedValue({
    scope: "run",
    pipeline: {
      name: "test",
      version: "1.0",
      variables: {},
      nodes: [],
      edges: [],
    },
    prompts: {},
    diagnostics: [],
  }),
  savePipeline: vi.fn().mockResolvedValue(undefined),
  saveRunPipeline: vi.fn().mockResolvedValue(undefined),
}));

const mockSavePipeline = vi.mocked(savePipeline);

function makePipeline(
  nodes: NodeDef[] = [],
  edges: EdgeDef[] = [],
): PipelineDef {
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
        diagnostics: [],
        dirty: false,
        externalDirty: false,
      },
    ],
    activeTabId: "test-tab",
    selection: { kind: "none", id: null },
  });
}

function seedTab(id = "test-pipeline", dirty = true) {
  useEditStore.setState({
    openTabs: [
      {
        id,
        scope: "repo",
        pipeline: {
          name: "test",
          version: "1.0",
          variables: {},
          nodes: [],
          edges: [],
        },
        prompts: {},
        diagnostics: [],
        dirty,
        externalDirty: false,
      },
    ],
    activeTabId: id,
    selection: { kind: "none", id: null },
  });
}

beforeEach(() => {
  useEditStore.setState({
    pipelines: [],
    openTabs: [],
    activeTabId: null,
    selection: { kind: "none", id: null },
    lastSavedAt: {},
  });
  vi.clearAllMocks();
});

describe("addNode", () => {
  it("adds a node to the active pipeline", () => {
    seedTabWithPipeline(makePipeline());

    const node = makeNode({ id: "abc12345", name: "worker" });
    useEditStore.getState().addNode(node);

    const tab = useEditStore.getState().openTabs[0];
    expect(tab.pipeline.nodes).toHaveLength(1);
    expect(tab.pipeline.nodes[0].id).toBe("abc12345");
    expect(tab.pipeline.nodes[0].name).toBe("worker");
  });
});

describe("duplicateNode", () => {
  it("generates a new id different from the original", () => {
    const original = makeNode({ id: "orig1234", name: "my-node" });
    seedTabWithPipeline(makePipeline([original]));

    useEditStore.getState().duplicateNode("orig1234");

    const tab = useEditStore.getState().openTabs[0];
    expect(tab.pipeline.nodes).toHaveLength(2);
    const dup = tab.pipeline.nodes[1];
    expect(dup.id).not.toBe("orig1234");
    expect(dup.id).toHaveLength(8);
    expect(dup.name).toBe("my-node copy");
  });

  it("generates unique ids across multiple duplications", () => {
    const original = makeNode({ id: "orig1234", name: "worker" });
    seedTabWithPipeline(makePipeline([original]));

    useEditStore.getState().duplicateNode("orig1234");
    useEditStore.getState().duplicateNode("orig1234");

    const tab = useEditStore.getState().openTabs[0];
    const ids = tab.pipeline.nodes.map((n) => n.id);
    const uniqueIds = new Set(ids);
    expect(uniqueIds.size).toBe(3);
  });
});

describe("updateNode with name", () => {
  it("updates node name without affecting edges", () => {
    const nodeA = makeNode({ id: "aaaaaaaa", name: "Alpha" });
    const nodeB = makeNode({ id: "bbbbbbbb", name: "Beta" });
    const edge: EdgeDef = {
      source: { node: "aaaaaaaa", port: "out" },
      target: { node: "bbbbbbbb", port: "in" },
    };
    seedTabWithPipeline(makePipeline([nodeA, nodeB], [edge]));

    useEditStore.getState().updateNode("aaaaaaaa", { name: "Renamed" });

    const tab = useEditStore.getState().openTabs[0];
    expect(tab.pipeline.nodes[0].name).toBe("Renamed");
    expect(tab.pipeline.edges[0].source.node).toBe("aaaaaaaa");
    expect(tab.pipeline.edges[0].target).toEqual({ node: "bbbbbbbb", port: "in" });
  });

  it("does not cascade name changes to edges", () => {
    const node = makeNode({ id: "cccccccc", name: "Original" });
    const edge: EdgeDef = {
      source: { node: "cccccccc", port: "out" },
      target: { node: "end", port: "result" },
      reason: "done",
    };
    seedTabWithPipeline(makePipeline([node], [edge]));

    useEditStore.getState().updateNode("cccccccc", { name: "New Name" });

    const tab = useEditStore.getState().openTabs[0];
    expect(tab.pipeline.edges[0].source.node).toBe("cccccccc");
  });
});

describe("editStore.save", () => {
  it("resolves with dirty === false after successful save", async () => {
    seedTab("p1", true);
    expect(useEditStore.getState().openTabs[0].dirty).toBe(true);

    await useEditStore.getState().save("p1");

    const tab = useEditStore.getState().openTabs.find((t) => t.id === "p1");
    expect(tab?.dirty).toBe(false);
  });

  it("sets lastSavedAt timestamp on successful save", async () => {
    seedTab("p1", true);
    const before = Date.now();

    await useEditStore.getState().save("p1");

    const ts = useEditStore.getState().lastSavedAt["p1"];
    expect(ts).toBeGreaterThanOrEqual(before);
    expect(ts).toBeLessThanOrEqual(Date.now());
  });

  it("does not set lastSavedAt when save fails", async () => {
    seedTab("p1", true);
    mockSavePipeline.mockImplementationOnce(() => Promise.reject(new Error("fail")));

    await useEditStore.getState().save("p1");

    expect(useEditStore.getState().lastSavedAt["p1"]).toBeUndefined();
  });
});

describe("editStore.flushPendingSaves", () => {
  it("resolves only after all dirty tabs are clean", async () => {
    useEditStore.setState({
      openTabs: [
        {
          id: "a",
          scope: "repo",
          pipeline: { name: "a", version: "1.0", variables: {}, nodes: [], edges: [] },
          prompts: {},
          diagnostics: [],
          dirty: true,
          externalDirty: false,
        },
        {
          id: "b",
          scope: "repo",
          pipeline: { name: "b", version: "1.0", variables: {}, nodes: [], edges: [] },
          prompts: {},
          diagnostics: [],
          dirty: true,
          externalDirty: false,
        },
        {
          id: "c",
          scope: "repo",
          pipeline: { name: "c", version: "1.0", variables: {}, nodes: [], edges: [] },
          prompts: {},
          diagnostics: [],
          dirty: false,
          externalDirty: false,
        },
      ],
      activeTabId: "a",
      selection: { kind: "none", id: null },
    });

    await useEditStore.getState().flushPendingSaves();

    const tabs = useEditStore.getState().openTabs;
    expect(tabs.every((t) => t.dirty === false)).toBe(true);
  });

  it("resolves immediately when no tabs are dirty", async () => {
    seedTab("p1", false);

    await useEditStore.getState().flushPendingSaves();

    expect(useEditStore.getState().openTabs[0].dirty).toBe(false);
  });

  it("saves only dirty tabs, not clean ones", async () => {
    useEditStore.setState({
      openTabs: [
        {
          id: "dirty-one",
          scope: "repo",
          pipeline: { name: "d", version: "1.0", variables: {}, nodes: [], edges: [] },
          prompts: {},
          diagnostics: [],
          dirty: true,
          externalDirty: false,
        },
        {
          id: "clean-one",
          scope: "repo",
          pipeline: { name: "c", version: "1.0", variables: {}, nodes: [], edges: [] },
          prompts: {},
          diagnostics: [],
          dirty: false,
          externalDirty: false,
        },
      ],
      activeTabId: "dirty-one",
      selection: { kind: "none", id: null },
    });

    await useEditStore.getState().flushPendingSaves();

    expect(mockSavePipeline).toHaveBeenCalledTimes(1);
    expect(mockSavePipeline.mock.calls[0][0]).toBe("dirty-one");
  });
});

describe("serializePipeline (via save) emits Loop max_iter", () => {
  it("includes max_iter in YAML for loop nodes (numeric)", async () => {
    const loopNode: NodeDef = {
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
      view: { x: 100, y: 100 },
      max_iter: 7,
    };
    seedTabWithPipeline(makePipeline([loopNode]));

    await useEditStore.getState().save("test-tab");

    expect(mockSavePipeline).toHaveBeenCalledTimes(1);
    const yaml = mockSavePipeline.mock.calls[0][1];
    expect(yaml).toMatch(/max_iter:\s*7/);
  });

  it("includes max_iter as variable reference (string)", async () => {
    const loopNode: NodeDef = {
      id: "loop1",
      name: "review-loop",
      type: "loop",
      inputs: [{ name: "in", repeated: false }],
      outputs: [{ name: "body", repeated: false }],
      interactive: false,
      view: { x: 0, y: 0 },
      max_iter: "$max_iter_review",
    };
    seedTabWithPipeline(makePipeline([loopNode]));

    await useEditStore.getState().save("test-tab");

    const yaml = mockSavePipeline.mock.calls[0][1];
    expect(yaml).toContain("max_iter:");
    expect(yaml).toContain("$max_iter_review");
  });

  it("does not emit max_iter for non-loop nodes", async () => {
    const docNode = makeNode({ id: "doc1", type: "doc-only" });
    seedTabWithPipeline(makePipeline([docNode]));

    await useEditStore.getState().save("test-tab");

    const yaml = mockSavePipeline.mock.calls[0][1];
    expect(yaml).not.toMatch(/max_iter/);
  });
});

describe("mutations set dirty without auto-saving", () => {
  it("addNode sets dirty but does not trigger save", async () => {
    seedTab("p1", false);

    useEditStore.getState().addNode({
      id: "new-node",
      type: "doc-only",
      inputs: [],
      outputs: [],
      interactive: false,
    });

    expect(useEditStore.getState().openTabs[0].dirty).toBe(true);

    await new Promise((r) => setTimeout(r, 2000));
    expect(mockSavePipeline).not.toHaveBeenCalled();
  });
});

const mockFetchPipeline = vi.mocked(fetchPipeline);

const EXTERNAL_PIPELINE: PipelineDef = {
  name: "externally-modified",
  version: "2.0",
  variables: {},
  nodes: [makeNode({ id: "ext-node", name: "External" })],
  edges: [],
};

describe("reloadPipeline conflict detection", () => {
  it("silently re-renders when tab is NOT dirty", async () => {
    seedTab("my-pipe", false);

    mockFetchPipeline.mockResolvedValueOnce({
      id: "my-pipe",
      scope: "repo",
      path: "/test/my-pipe.yaml",
      yaml: "",
      pipeline: EXTERNAL_PIPELINE,
      prompts: { "ext-node": "external prompt" },
      diagnostics: [],
    });

    await useEditStore.getState().reloadPipeline("my-pipe");

    const tab = useEditStore.getState().openTabs[0];
    expect(tab.pipeline.name).toBe("externally-modified");
    expect(tab.dirty).toBe(false);
    expect(tab.externalDirty).toBe(true);
    expect(tab.conflict).toBeUndefined();
  });

  it("sets conflict state instead of overwriting when tab IS dirty", async () => {
    seedTab("my-pipe", true);

    mockFetchPipeline.mockResolvedValueOnce({
      id: "my-pipe",
      scope: "repo",
      path: "/test/my-pipe.yaml",
      yaml: "",
      pipeline: EXTERNAL_PIPELINE,
      prompts: { "ext-node": "external prompt" },
      diagnostics: ["diag1"],
    });

    await useEditStore.getState().reloadPipeline("my-pipe");

    const tab = useEditStore.getState().openTabs[0];
    // Canvas should NOT be overwritten
    expect(tab.pipeline.name).toBe("test");
    expect(tab.dirty).toBe(true);
    // Conflict data should be stored
    expect(tab.conflict).toBeDefined();
    expect(tab.conflict!.pipeline.name).toBe("externally-modified");
    expect(tab.conflict!.prompts["ext-node"]).toBe("external prompt");
    expect(tab.conflict!.diagnostics).toEqual(["diag1"]);
  });
});

describe("resolveConflict", () => {
  it("'keep' discards external data and keeps canvas", () => {
    seedTab("my-pipe", true);

    // Simulate conflict state
    useEditStore.setState((s) => ({
      openTabs: s.openTabs.map((t) =>
        t.id === "my-pipe"
          ? {
              ...t,
              conflict: {
                pipeline: EXTERNAL_PIPELINE,
                prompts: { "ext-node": "ext" },
                diagnostics: [],
              },
            }
          : t,
      ),
    }));

    useEditStore.getState().resolveConflict("my-pipe", "keep");

    const tab = useEditStore.getState().openTabs[0];
    expect(tab.conflict).toBeUndefined();
    expect(tab.pipeline.name).toBe("test");
    expect(tab.dirty).toBe(true);
  });

  it("'take' applies external data and clears dirty+conflict", () => {
    seedTab("my-pipe", true);

    useEditStore.setState((s) => ({
      openTabs: s.openTabs.map((t) =>
        t.id === "my-pipe"
          ? {
              ...t,
              conflict: {
                pipeline: EXTERNAL_PIPELINE,
                prompts: { "ext-node": "ext" },
                diagnostics: ["d1"],
              },
            }
          : t,
      ),
    }));

    useEditStore.getState().resolveConflict("my-pipe", "take");

    const tab = useEditStore.getState().openTabs[0];
    expect(tab.conflict).toBeUndefined();
    expect(tab.pipeline.name).toBe("externally-modified");
    expect(tab.prompts["ext-node"]).toBe("ext");
    expect(tab.diagnostics).toEqual(["d1"]);
    expect(tab.dirty).toBe(false);
  });
});
