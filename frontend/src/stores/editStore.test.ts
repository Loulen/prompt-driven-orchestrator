import { describe, it, expect, beforeEach, vi } from "vitest";
import { useEditStore, serializePipeline } from "./editStore";
import { savePipeline, fetchPipeline, saveRunPipeline } from "../api";
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
const mockSaveRunPipeline = vi.mocked(saveRunPipeline);

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

describe("addEdge auto-materializes a bounded loop region on a cycle (ADR-0011 / #166)", () => {
  function edge(s: string, t: string): EdgeDef {
    return { source: { node: s, port: "out" }, target: { node: t, port: "in" } };
  }

  it("materializes a bounded region over both members when a back-edge closes a two-node cycle", () => {
    const a = makeNode({ id: "aaaa1111", name: "impl" });
    const b = makeNode({ id: "bbbb2222", name: "rev" });
    // Forward edge a -> b already exists; drawing b -> a closes the cycle.
    seedTabWithPipeline(makePipeline([a, b], [edge("aaaa1111", "bbbb2222")]));

    useEditStore.getState().addEdge(edge("bbbb2222", "aaaa1111"));

    const tab = useEditStore.getState().openTabs[0];
    expect(tab.pipeline.loops).toHaveLength(1);
    const region = tab.pipeline.loops![0];
    expect(region.kind).toBe("bounded");
    expect(new Set(region.members)).toEqual(new Set(["aaaa1111", "bbbb2222"]));
  });

  it("creates no region for an acyclic edge", () => {
    const a = makeNode({ id: "aaaa1111", name: "first" });
    const b = makeNode({ id: "bbbb2222", name: "second" });
    seedTabWithPipeline(makePipeline([a, b]));

    useEditStore.getState().addEdge(edge("aaaa1111", "bbbb2222"));

    const tab = useEditStore.getState().openTabs[0];
    expect(tab.pipeline.loops ?? []).toHaveLength(0);
  });

  it("materializes a single-member region for a self-edge", () => {
    const a = makeNode({ id: "aaaa1111", name: "self-looper" });
    seedTabWithPipeline(makePipeline([a]));

    useEditStore.getState().addEdge(edge("aaaa1111", "aaaa1111"));

    const tab = useEditStore.getState().openTabs[0];
    expect(tab.pipeline.loops).toHaveLength(1);
    expect(tab.pipeline.loops![0].members).toEqual(["aaaa1111"]);
    expect(tab.pipeline.loops![0].kind).toBe("bounded");
  });

  it("does not add a second region when a cycle's member set is already covered", () => {
    const a = makeNode({ id: "aaaa1111", name: "impl" });
    const b = makeNode({ id: "bbbb2222", name: "rev" });
    const pipeline = makePipeline(
      [a, b],
      [edge("aaaa1111", "bbbb2222"), edge("bbbb2222", "aaaa1111")],
    );
    // The {a,b} cycle is already a named region.
    pipeline.loops = [
      { id: "review_loop", kind: "bounded", members: ["aaaa1111", "bbbb2222"], max_iter: 3 },
    ];
    seedTabWithPipeline(pipeline);

    // Draw a redundant edge among the same members (still the same SCC).
    useEditStore.getState().addEdge(edge("aaaa1111", "bbbb2222"));

    const tab = useEditStore.getState().openTabs[0];
    expect(tab.pipeline.loops).toHaveLength(1);
    expect(tab.pipeline.loops![0].id).toBe("review_loop");
  });

  it("uses a deterministic generated id matching the daemon's loop-<hash> form", () => {
    const a = makeNode({ id: "aaaa1111", name: "impl" });
    const b = makeNode({ id: "bbbb2222", name: "rev" });
    seedTabWithPipeline(makePipeline([a, b], [edge("aaaa1111", "bbbb2222")]));

    useEditStore.getState().addEdge(edge("bbbb2222", "aaaa1111"));

    const tab = useEditStore.getState().openTabs[0];
    // Deterministic FNV-1a over the sorted members, matching the daemon's
    // `loop_region::generated_region_id` so the editor and engine agree on the
    // region id for the same member set.
    expect(tab.pipeline.loops![0].id).toBe("loop-aae2153b41ac0dfd");
    expect(tab.pipeline.loops![0].max_iter).toBe(5);
  });

  it("serializes the auto-materialized region into the loops: YAML block", () => {
    const a = makeNode({ id: "aaaa1111", name: "impl" });
    const b = makeNode({ id: "bbbb2222", name: "rev" });
    seedTabWithPipeline(makePipeline([a, b], [edge("aaaa1111", "bbbb2222")]));

    useEditStore.getState().addEdge(edge("bbbb2222", "aaaa1111"));

    const yaml = serializePipeline(useEditStore.getState().openTabs[0].pipeline);
    expect(yaml).toContain("loops:");
    expect(yaml).toContain("id: loop-aae2153b41ac0dfd");
    expect(yaml).toContain("kind: bounded");
    expect(yaml).toContain("max_iter: 5");
    expect(yaml).toContain("aaaa1111");
    expect(yaml).toContain("bbbb2222");
  });

  it("captures every member of a three-node cycle, ordered by node position", () => {
    const a = makeNode({ id: "aaaa1111", name: "a" });
    const b = makeNode({ id: "bbbb2222", name: "b" });
    const c = makeNode({ id: "cccc3333", name: "c" });
    // a -> b -> c already wired; closing c -> a forms a 3-node SCC.
    seedTabWithPipeline(
      makePipeline([a, b, c], [edge("aaaa1111", "bbbb2222"), edge("bbbb2222", "cccc3333")]),
    );

    useEditStore.getState().addEdge(edge("cccc3333", "aaaa1111"));

    const tab = useEditStore.getState().openTabs[0];
    expect(tab.pipeline.loops).toHaveLength(1);
    // Ordered by node position (nodes array order), for deterministic YAML.
    expect(tab.pipeline.loops![0].members).toEqual([
      "aaaa1111",
      "bbbb2222",
      "cccc3333",
    ]);
  });
});

describe("edge selection (ADR-0011 edge detail panel, #147)", () => {
  it("selects an edge by index", () => {
    useEditStore.getState().setSelection({ kind: "edge", id: null, edgeIndex: 2 });
    const sel = useEditStore.getState().selection;
    expect(sel.kind).toBe("edge");
    expect(sel.edgeIndex).toBe(2);
  });

  it("clearing selection back to none drops the edge index", () => {
    useEditStore.getState().setSelection({ kind: "edge", id: null, edgeIndex: 0 });
    useEditStore.getState().setSelection({ kind: "none", id: null });
    expect(useEditStore.getState().selection.kind).toBe("none");
    expect(useEditStore.getState().selection.edgeIndex).toBeUndefined();
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

describe("serializePipeline (via save) emits structural node fields", () => {
  // The legacy Loop node carried a node-level `max_iter` (#171 removed it). A
  // bounded loop is now a `loops:` region whose `max_iter` is serialized on the
  // region, not on any node — so no node ever emits `max_iter`.
  it("never emits a node-level max_iter (loop node removed, #171)", async () => {
    const docNode = makeNode({ id: "doc1", type: "doc-only" });
    seedTabWithPipeline(makePipeline([docNode]));

    await useEditStore.getState().save("test-tab");

    const yaml = mockSavePipeline.mock.calls[0][1];
    expect(yaml).not.toMatch(/max_iter/);
  });

  // ForEach-node `over` serialization and `over`-reset-on-edge-delete tests were
  // removed with the ForEach node type (#151): a collection's `over` driver now
  // lives on the `loops:` region, not on any node.
});

describe("save error storage", () => {
  it("stores a structured save error on the tab when save fails", async () => {
    seedTab("p1", true);
    mockSavePipeline.mockImplementationOnce(() =>
      Promise.reject({ message: "invalid YAML: missing field 'name'", line: 42 }),
    );

    await useEditStore.getState().save("p1");

    const tab = useEditStore.getState().openTabs.find((t) => t.id === "p1");
    expect(tab?.saveError).toBeDefined();
    expect(tab?.saveError?.message).toBe("invalid YAML: missing field 'name'");
    expect(tab?.saveError?.line).toBe(42);
  });

  it("keeps dirty flag true when save fails", async () => {
    seedTab("p1", true);
    mockSavePipeline.mockImplementationOnce(() =>
      Promise.reject({ message: "fail" }),
    );

    await useEditStore.getState().save("p1");

    const tab = useEditStore.getState().openTabs.find((t) => t.id === "p1");
    expect(tab?.dirty).toBe(true);
  });

  it("clears save error on successful save", async () => {
    seedTab("p1", true);
    // First fail
    mockSavePipeline.mockImplementationOnce(() =>
      Promise.reject({ message: "fail" }),
    );
    await useEditStore.getState().save("p1");
    expect(useEditStore.getState().openTabs[0].saveError).toBeDefined();

    // Make dirty again and succeed
    useEditStore.setState((s) => ({
      openTabs: s.openTabs.map((t) => (t.id === "p1" ? { ...t, dirty: true } : t)),
    }));
    mockSavePipeline.mockImplementationOnce(() => Promise.resolve(undefined));
    await useEditStore.getState().save("p1");

    const tab = useEditStore.getState().openTabs.find((t) => t.id === "p1");
    expect(tab?.saveError).toBeUndefined();
  });

  it("clearSaveError removes the error from the tab", () => {
    seedTab("p1", true);
    useEditStore.setState((s) => ({
      openTabs: s.openTabs.map((t) =>
        t.id === "p1" ? { ...t, saveError: { message: "fail", line: 1 } } : t,
      ),
    }));

    useEditStore.getState().clearSaveError("p1");

    const tab = useEditStore.getState().openTabs.find((t) => t.id === "p1");
    expect(tab?.saveError).toBeUndefined();
  });

  it("stores error without line when line is not present", async () => {
    seedTab("p1", true);
    mockSavePipeline.mockImplementationOnce(() =>
      Promise.reject({ message: "write failed: permission denied" }),
    );

    await useEditStore.getState().save("p1");

    const tab = useEditStore.getState().openTabs.find((t) => t.id === "p1");
    expect(tab?.saveError).toBeDefined();
    expect(tab?.saveError?.message).toBe("write failed: permission denied");
    expect(tab?.saveError?.line).toBeUndefined();
  });

  it("silently closes a run-scoped tab when the daemon returns 404", async () => {
    const tabId = "__run__archived-run-id";
    useEditStore.setState({
      openTabs: [
        {
          id: tabId,
          scope: "run",
          pipeline: { name: "test", version: "1.0", variables: {}, nodes: [], edges: [] },
          prompts: {},
          diagnostics: [],
          dirty: true,
          externalDirty: false,
          runId: "archived-run-id",
        },
      ],
      activeTabId: tabId,
      selection: { kind: "none", id: null },
      lastSavedAt: { [tabId]: 123 },
    });
    mockSaveRunPipeline.mockImplementationOnce(() =>
      Promise.reject({
        message: "PUT /runs/archived-run-id/pipeline failed: 404",
        status: 404,
      }),
    );

    await useEditStore.getState().save(tabId);

    const state = useEditStore.getState();
    expect(state.openTabs.find((t) => t.id === tabId)).toBeUndefined();
    expect(state.activeTabId).toBeNull();
    expect(state.lastSavedAt[tabId]).toBeUndefined();
  });

  it("still surfaces non-404 errors for run-scoped tabs", async () => {
    const tabId = "__run__live-run-id";
    useEditStore.setState({
      openTabs: [
        {
          id: tabId,
          scope: "run",
          pipeline: { name: "test", version: "1.0", variables: {}, nodes: [], edges: [] },
          prompts: {},
          diagnostics: [],
          dirty: true,
          externalDirty: false,
          runId: "live-run-id",
        },
      ],
      activeTabId: tabId,
      selection: { kind: "none", id: null },
      lastSavedAt: {},
    });
    mockSaveRunPipeline.mockImplementationOnce(() =>
      Promise.reject({ message: "boom", status: 500 }),
    );

    await useEditStore.getState().save(tabId);

    const tab = useEditStore.getState().openTabs.find((t) => t.id === tabId);
    expect(tab).toBeDefined();
    expect(tab?.saveError?.message).toBe("boom");
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

describe("serializePipeline round-trip: YAML structural correctness", () => {
  function makeFullPipeline(extraNodes: NodeDef[], edges: EdgeDef[] = []): PipelineDef {
    const start: NodeDef = {
      id: "start", name: "Start", type: "start",
      inputs: [], outputs: [{ name: "user_prompt", repeated: false, side: "right" }],
      interactive: false, view: { x: 0, y: 0 },
    };
    const end: NodeDef = {
      id: "end", name: "End", type: "end",
      inputs: [{ name: "result", repeated: false, side: "left" }], outputs: [],
      interactive: false, view: { x: 400, y: 0 },
    };
    return {
      name: "round-trip-test", version: "1.0", variables: {},
      nodes: [start, ...extraNodes, end], edges,
    };
  }

  it("serializes a minimal start+end pipeline to parseable YAML", () => {
    const pipeline = makeFullPipeline([]);
    const yaml = serializePipeline(pipeline);
    expect(yaml).toContain("name: round-trip-test");
    expect(yaml).toContain("type: start");
    expect(yaml).toContain("type: end");
  });

  it("serializes a bounded loops: region block (ADR-0011 / #148)", () => {
    const impl: NodeDef = {
      id: "impl", name: "implementer", type: "code-mutating",
      inputs: [], outputs: [{ name: "code", repeated: false, side: "right" }],
      interactive: false, view: { x: 200, y: 0 },
    };
    const rev: NodeDef = {
      id: "rev", name: "reviewer", type: "doc-only",
      inputs: [], outputs: [{ name: "review", repeated: false, side: "right" }],
      interactive: false, view: { x: 300, y: 0 },
    };
    const pipeline = makeFullPipeline([impl, rev]);
    pipeline.loops = [
      { id: "review_loop", kind: "bounded", members: ["impl", "rev"], max_iter: 3 },
    ];
    const yaml = serializePipeline(pipeline);
    expect(yaml).toContain("loops:");
    expect(yaml).toContain("id: review_loop");
    expect(yaml).toContain("kind: bounded");
    expect(yaml).toContain("max_iter: 3");
    // members listed
    expect(yaml).toMatch(/members:/);
  });

  it("omits the loops: block when there are no regions", () => {
    const yaml = serializePipeline(makeFullPipeline([]));
    expect(yaml).not.toContain("loops:");
  });

  it("emits prompt_required: false for prompt-optional pipelines (#158)", () => {
    const pipeline = makeFullPipeline([]);
    pipeline.prompt_required = false;
    const yaml = serializePipeline(pipeline);
    expect(yaml).toContain("prompt_required: false");
  });

  it("omits prompt_required when prompt-required (the default, #158)", () => {
    const requiredExplicit = makeFullPipeline([]);
    requiredExplicit.prompt_required = true;
    expect(serializePipeline(requiredExplicit)).not.toContain("prompt_required");

    // Absent flag is the prompt-required default → still omitted.
    const absent = makeFullPipeline([]);
    expect(serializePipeline(absent)).not.toContain("prompt_required");
  });

  it("serializes output port with frontmatter at correct indentation", () => {
    const reviewer: NodeDef = {
      id: "reviewer", name: "reviewer", type: "doc-only",
      inputs: [{ name: "code", repeated: false, side: "left" }],
      outputs: [{
        name: "review", repeated: false, side: "right",
        frontmatter: {
          verdict: { type: "enum", allowed: ["PASS", "FAIL"] },
        },
      }],
      interactive: false, view: { x: 200, y: 0 },
    };
    const yaml = serializePipeline(makeFullPipeline([reviewer]));

    // The frontmatter fields (type/allowed) must be siblings, not parent-child
    const lines = yaml.split("\n");
    const typeIdx = lines.findIndex((l) => l.includes("type: enum"));
    const allowedIdx = lines.findIndex((l) => l.includes("allowed:"));
    expect(typeIdx).toBeGreaterThan(-1);
    expect(allowedIdx).toBeGreaterThan(-1);

    // Both should have the same leading whitespace (they're siblings under verdict:)
    const typeIndent = lines[typeIdx].match(/^(\s*)/)?.[1].length ?? -1;
    const allowedIndent = lines[allowedIdx].match(/^(\s*)/)?.[1].length ?? -1;
    expect(typeIndent).toBe(allowedIndent);
  });

  it("serializes an edge when clause at correct indentation", () => {
    const gate: NodeDef = {
      id: "gate", name: "gate", type: "doc-only",
      inputs: [{ name: "in", repeated: false, side: "left" }],
      outputs: [{ name: "out", repeated: false, side: "right" }],
      interactive: false, view: { x: 200, y: 0 },
    };
    const yaml = serializePipeline(
      makeFullPipeline([gate], [
        {
          source: { node: "gate", port: "out" },
          target: { node: "end", port: "result" },
          when: { verdict: { eq: "PASS" }, score: { gte: 7 } },
        },
      ]),
    );

    const lines = yaml.split("\n");
    // Find verdict and score lines under when: — they must be at same indent
    const verdictIdx = lines.findIndex((l) => l.includes("verdict:"));
    const scoreIdx = lines.findIndex((l) => l.includes("score:"));
    expect(verdictIdx).toBeGreaterThan(-1);
    expect(scoreIdx).toBeGreaterThan(-1);

    const verdictIndent = lines[verdictIdx].match(/^(\s*)/)?.[1].length ?? -1;
    const scoreIndent = lines[scoreIdx].match(/^(\s*)/)?.[1].length ?? -1;
    expect(verdictIndent).toBe(scoreIndent);
  });

  it("serializes a manual edge's mode and waypoints (shareable routing, #154)", () => {
    const gate: NodeDef = {
      id: "gate", name: "gate", type: "doc-only",
      inputs: [{ name: "in", repeated: false, side: "left" }],
      outputs: [{ name: "out", repeated: false, side: "right" }],
      interactive: false, view: { x: 200, y: 0 },
    };
    const yaml = serializePipeline(
      makeFullPipeline([gate], [
        {
          source: { node: "gate", port: "out" },
          target: { node: "end", port: "result" },
          mode: "manual",
          waypoints: [
            { x: 120, y: 40 },
            { x: 120, y: 220 },
          ],
        },
      ]),
    );
    expect(yaml).toContain("mode: manual");
    expect(yaml).toContain("waypoints:");
    // The coordinates survive so the route travels with a shared pipeline.
    expect(yaml).toContain("x: 120");
    expect(yaml).toContain("y: 40");
    expect(yaml).toContain("y: 220");
  });

  it("omits routing fields for an auto edge (no waypoints stored, #154)", () => {
    const gate: NodeDef = {
      id: "gate", name: "gate", type: "doc-only",
      inputs: [{ name: "in", repeated: false, side: "left" }],
      outputs: [{ name: "out", repeated: false, side: "right" }],
      interactive: false, view: { x: 200, y: 0 },
    };
    const yaml = serializePipeline(
      makeFullPipeline([gate], [
        {
          source: { node: "gate", port: "out" },
          target: { node: "end", port: "result" },
          mode: "auto",
        },
      ]),
    );
    // Auto edges recompute deterministically — nothing routing-related persists.
    expect(yaml).not.toContain("mode:");
    expect(yaml).not.toContain("waypoints:");
  });

  it("serializes an edge's target_side so the drop-position anchor survives reload (#168)", () => {
    const impl: NodeDef = {
      id: "impl", name: "impl", type: "code-mutating",
      inputs: [], outputs: [{ name: "out", repeated: false, side: "right" }],
      interactive: false, view: { x: 200, y: 0 },
    };
    const yaml = serializePipeline(
      makeFullPipeline([impl], [
        {
          source: { node: "start", port: "user_prompt" },
          target: { node: "impl", port: "user_prompt" },
          target_side: "top",
        },
      ]),
    );
    expect(yaml).toContain("target_side: top");
  });

  it("omits target_side for a left-anchored (legacy) edge (#168)", () => {
    const impl: NodeDef = {
      id: "impl", name: "impl", type: "code-mutating",
      inputs: [], outputs: [{ name: "out", repeated: false, side: "right" }],
      interactive: false, view: { x: 200, y: 0 },
    };
    const yaml = serializePipeline(
      makeFullPipeline([impl], [
        {
          source: { node: "start", port: "user_prompt" },
          target: { node: "impl", port: "user_prompt" },
        },
      ]),
    );
    expect(yaml).not.toContain("target_side:");
  });

  it("serializes multi-field frontmatter with all fields at same depth", () => {
    const node: NodeDef = {
      id: "multi", name: "multi", type: "doc-only",
      inputs: [{ name: "in", repeated: false }],
      outputs: [{
        name: "out", repeated: false,
        frontmatter: {
          verdict: { type: "enum", allowed: ["PASS", "FAIL"] },
          score: { type: "int" },
          summary: { type: "string" },
        },
      }],
      interactive: false, view: { x: 200, y: 0 },
    };
    const yaml = serializePipeline(makeFullPipeline([node]));

    const lines = yaml.split("\n");
    const verdictLine = lines.find((l) => /^\s+verdict:/.test(l));
    const scoreLine = lines.find((l) => /^\s+score:/.test(l));
    const summaryLine = lines.find((l) => /^\s+summary:/.test(l));

    expect(verdictLine).toBeDefined();
    expect(scoreLine).toBeDefined();
    expect(summaryLine).toBeDefined();

    const indent = (l: string) => l.match(/^(\s*)/)?.[1].length ?? -1;
    expect(indent(verdictLine!)).toBe(indent(scoreLine!));
    expect(indent(scoreLine!)).toBe(indent(summaryLine!));
  });

  it("serializes a deeply nested edge when clause with in-predicate correctly", () => {
    const gate: NodeDef = {
      id: "gate", name: "gate", type: "doc-only",
      inputs: [{ name: "in", repeated: false, side: "left" }],
      outputs: [{ name: "out", repeated: false, side: "right" }],
      interactive: false, view: { x: 200, y: 0 },
    };
    const yaml = serializePipeline(
      makeFullPipeline([gate], [
        {
          source: { node: "gate", port: "out" },
          target: { node: "end", port: "result" },
          when: { verdict: { in: ["PASS", "APPROVED"] } },
        },
      ]),
    );

    // The YAML must not contain excessive indentation (more than 16 spaces
    // for any line would indicate double-indent bug)
    const lines = yaml.split("\n");
    for (const line of lines) {
      const leadingSpaces = line.match(/^(\s*)/)?.[1].length ?? 0;
      expect(leadingSpaces).toBeLessThan(16);
    }
  });

  it("serializes pipeline with variables correctly", () => {
    const pipeline: PipelineDef = {
      name: "vars-test", version: "1.0",
      variables: {
        max_iter: { type: "int", default: 5 },
        threshold: { type: "float", default: 0.8 },
      },
      nodes: [
        {
          id: "start", name: "Start", type: "start",
          inputs: [], outputs: [{ name: "user_prompt", repeated: false, side: "right" }],
          interactive: false, view: { x: 0, y: 0 },
        },
        {
          id: "end", name: "End", type: "end",
          inputs: [{ name: "result", repeated: false, side: "left" }], outputs: [],
          interactive: false, view: { x: 400, y: 0 },
        },
      ],
      edges: [],
    };
    const yaml = serializePipeline(pipeline);
    expect(yaml).toContain("variables:");
    expect(yaml).toContain("max_iter: 5");
    expect(yaml).toContain("threshold: 0.8");
  });
});

describe("serializePipeline persists edge when/else (ADR-0011)", () => {
  function makeEdgePipeline(edges: EdgeDef[]): PipelineDef {
    return {
      name: "edge-when-test",
      version: "1.0",
      variables: {},
      nodes: [
        {
          id: "reviewer", name: "reviewer", type: "doc-only",
          inputs: [{ name: "task", repeated: false, side: "left" }],
          outputs: [{ name: "verdict", repeated: false, side: "right" }],
          interactive: false, view: { x: 0, y: 0 },
        },
        {
          id: "impl", name: "impl", type: "code-mutating",
          inputs: [{ name: "review", repeated: false, side: "left" }],
          outputs: [{ name: "diff", repeated: false, side: "right" }],
          interactive: false, view: { x: 200, y: 0 },
        },
      ],
      edges,
    };
  }

  it("emits the when clause on a guarded edge", () => {
    const yaml = serializePipeline(
      makeEdgePipeline([
        {
          source: { node: "reviewer", port: "verdict" },
          target: { node: "impl", port: "review" },
          when: { verdict: { eq: "FAIL" } },
        },
      ]),
    );
    expect(yaml).toContain("when:");
    expect(yaml).toContain("verdict:");
    expect(yaml).toContain("eq: FAIL");
  });

  it("emits a canonical boolean (not a string) for a bool when value", () => {
    const yaml = serializePipeline(
      makeEdgePipeline([
        {
          source: { node: "reviewer", port: "verdict" },
          target: { node: "impl", port: "review" },
          when: { is_blocking: { eq: true } },
        },
      ]),
    );
    // The value must be a YAML boolean `true`, never the string "true".
    expect(yaml).toMatch(/eq: true\b/);
    expect(yaml).not.toContain('eq: "true"');
  });

  it("emits else: true on a fallback edge", () => {
    const yaml = serializePipeline(
      makeEdgePipeline([
        {
          source: { node: "reviewer", port: "verdict" },
          target: { node: "impl", port: "review" },
          else: true,
        },
      ]),
    );
    expect(yaml).toContain("else: true");
  });

  it("omits when/else on an unconditional edge", () => {
    const yaml = serializePipeline(
      makeEdgePipeline([
        {
          source: { node: "reviewer", port: "verdict" },
          target: { node: "impl", port: "review" },
        },
      ]),
    );
    expect(yaml).not.toContain("when:");
    expect(yaml).not.toContain("else:");
  });
});

describe("serializePipeline persists port_type", () => {
  function makePipelineWithTypedPorts(): PipelineDef {
    const tester: NodeDef = {
      id: "9NOnrpKY",
      name: "Tester",
      type: "doc-only",
      inputs: [
        { name: "screens", repeated: false, side: "left", port_type: "image_list" },
      ],
      outputs: [
        { name: "screens-fixed", repeated: false, side: "right", port_type: "image_list" },
        { name: "report", repeated: false, side: "right" },
      ],
      interactive: false,
      view: { x: 200, y: 0 },
    };
    return {
      name: "typed-ports-test",
      version: "1.0",
      variables: {},
      nodes: [tester],
      edges: [],
    };
  }

  it("emits port_type: image_list for both input and output ports", () => {
    const yaml = serializePipeline(makePipelineWithTypedPorts());
    const occurrences = yaml.match(/port_type: image_list/g) ?? [];
    // One for the input port (screens), one for the output port (screens-fixed).
    expect(occurrences.length).toBe(2);
  });

  it("does not emit port_type for the default markdown type", () => {
    const yaml = serializePipeline(makePipelineWithTypedPorts());
    // The "report" output has no port_type set, so it must default to markdown
    // implicitly and never appear in the YAML.
    expect(yaml).not.toContain("port_type: markdown");
  });
});

describe("updateNode propagates port changes to edges", () => {
  it("renames edge source port when an output port is renamed", () => {
    const nodeA = makeNode({
      id: "aaaaaaaa",
      outputs: [{ name: "screenshots", repeated: false }],
    });
    const nodeB = makeNode({ id: "bbbbbbbb" });
    const edge: EdgeDef = {
      source: { node: "aaaaaaaa", port: "screenshots" },
      target: { node: "bbbbbbbb", port: "in" },
    };
    seedTabWithPipeline(makePipeline([nodeA, nodeB], [edge]));

    useEditStore.getState().updateNode("aaaaaaaa", {
      outputs: [{ name: "screen", repeated: false }],
    });

    const tab = useEditStore.getState().openTabs[0];
    expect(tab.pipeline.edges).toHaveLength(1);
    expect(tab.pipeline.edges[0].source.port).toBe("screen");
    expect(tab.pipeline.edges[0].target).toEqual({ node: "bbbbbbbb", port: "in" });
  });

  it("renames edge target port when an input port is renamed", () => {
    const nodeA = makeNode({ id: "aaaaaaaa" });
    const nodeB = makeNode({
      id: "bbbbbbbb",
      inputs: [{ name: "data", repeated: false }],
    });
    const edge: EdgeDef = {
      source: { node: "aaaaaaaa", port: "out" },
      target: { node: "bbbbbbbb", port: "data" },
    };
    seedTabWithPipeline(makePipeline([nodeA, nodeB], [edge]));

    useEditStore.getState().updateNode("bbbbbbbb", {
      inputs: [{ name: "payload", repeated: false }],
    });

    const tab = useEditStore.getState().openTabs[0];
    expect(tab.pipeline.edges).toHaveLength(1);
    expect(tab.pipeline.edges[0].target.port).toBe("payload");
  });

  it("removes edge when a connected output port is deleted", () => {
    const nodeA = makeNode({
      id: "aaaaaaaa",
      outputs: [
        { name: "out", repeated: false },
        { name: "screenshots", repeated: false },
      ],
    });
    const nodeB = makeNode({ id: "bbbbbbbb" });
    const edges: EdgeDef[] = [
      { source: { node: "aaaaaaaa", port: "out" }, target: { node: "bbbbbbbb", port: "in" } },
      { source: { node: "aaaaaaaa", port: "screenshots" }, target: { node: "bbbbbbbb", port: "in" } },
    ];
    seedTabWithPipeline(makePipeline([nodeA, nodeB], edges));

    useEditStore.getState().updateNode("aaaaaaaa", {
      outputs: [{ name: "out", repeated: false }],
    });

    const tab = useEditStore.getState().openTabs[0];
    expect(tab.pipeline.edges).toHaveLength(1);
    expect(tab.pipeline.edges[0].source.port).toBe("out");
  });

  it("removes edge when a connected input port is deleted", () => {
    const nodeA = makeNode({ id: "aaaaaaaa" });
    const nodeB = makeNode({
      id: "bbbbbbbb",
      inputs: [
        { name: "in", repeated: false },
        { name: "extra", repeated: false },
      ],
    });
    const edges: EdgeDef[] = [
      { source: { node: "aaaaaaaa", port: "out" }, target: { node: "bbbbbbbb", port: "in" } },
      { source: { node: "aaaaaaaa", port: "out" }, target: { node: "bbbbbbbb", port: "extra" } },
    ];
    seedTabWithPipeline(makePipeline([nodeA, nodeB], edges));

    useEditStore.getState().updateNode("bbbbbbbb", {
      inputs: [{ name: "in", repeated: false }],
    });

    const tab = useEditStore.getState().openTabs[0];
    expect(tab.pipeline.edges).toHaveLength(1);
    expect(tab.pipeline.edges[0].target.port).toBe("in");
  });

  it("does not affect edges on other nodes", () => {
    const nodeA = makeNode({
      id: "aaaaaaaa",
      outputs: [{ name: "out", repeated: false }],
    });
    const nodeB = makeNode({
      id: "bbbbbbbb",
      outputs: [{ name: "out", repeated: false }],
    });
    const nodeC = makeNode({ id: "cccccccc" });
    const edges: EdgeDef[] = [
      { source: { node: "aaaaaaaa", port: "out" }, target: { node: "cccccccc", port: "in" } },
      { source: { node: "bbbbbbbb", port: "out" }, target: { node: "cccccccc", port: "in" } },
    ];
    seedTabWithPipeline(makePipeline([nodeA, nodeB, nodeC], edges));

    useEditStore.getState().updateNode("aaaaaaaa", {
      outputs: [{ name: "result", repeated: false }],
    });

    const tab = useEditStore.getState().openTabs[0];
    expect(tab.pipeline.edges).toHaveLength(2);
    expect(tab.pipeline.edges[0].source.port).toBe("result");
    expect(tab.pipeline.edges[1].source.port).toBe("out");
  });

  // The "clears for-each over when deleting a port causes in-edge removal" test
  // was removed with the ForEach node type (#151) — its `over`-clearing side
  // effect no longer exists (a collection's `over` lives on the region). The
  // cascade-delete-edge-on-port-removal behaviour is covered by its own test
  // above.

  it("does not rename when old port name still exists in new array", () => {
    const node = makeNode({
      id: "aaaaaaaa",
      outputs: [
        { name: "alpha", repeated: false },
        { name: "beta", repeated: false },
      ],
    });
    const nodeB = makeNode({ id: "bbbbbbbb" });
    const edges: EdgeDef[] = [
      { source: { node: "aaaaaaaa", port: "alpha" }, target: { node: "bbbbbbbb", port: "in" } },
      { source: { node: "aaaaaaaa", port: "beta" }, target: { node: "bbbbbbbb", port: "in" } },
    ];
    seedTabWithPipeline(makePipeline([node, nodeB], edges));

    // Swap order: [beta, alpha] — same names, different indices
    useEditStore.getState().updateNode("aaaaaaaa", {
      outputs: [
        { name: "beta", repeated: false },
        { name: "alpha", repeated: false },
      ],
    });

    const tab = useEditStore.getState().openTabs[0];
    expect(tab.pipeline.edges).toHaveLength(2);
    expect(tab.pipeline.edges[0].source.port).toBe("alpha");
    expect(tab.pipeline.edges[1].source.port).toBe("beta");
  });
});
