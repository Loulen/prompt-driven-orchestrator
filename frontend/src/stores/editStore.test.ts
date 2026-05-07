import { describe, it, expect, vi, beforeEach } from "vitest";
import { useEditStore } from "./editStore";
import { savePipeline } from "../api";

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
  }),
  savePipeline: vi.fn().mockResolvedValue(undefined),
  saveRunPipeline: vi.fn().mockResolvedValue(undefined),
}));

const mockSavePipeline = vi.mocked(savePipeline);

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
          dirty: true,
          externalDirty: false,
        },
        {
          id: "b",
          scope: "repo",
          pipeline: { name: "b", version: "1.0", variables: {}, nodes: [], edges: [] },
          prompts: {},
          dirty: true,
          externalDirty: false,
        },
        {
          id: "c",
          scope: "repo",
          pipeline: { name: "c", version: "1.0", variables: {}, nodes: [], edges: [] },
          prompts: {},
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
          dirty: true,
          externalDirty: false,
        },
        {
          id: "clean-one",
          scope: "repo",
          pipeline: { name: "c", version: "1.0", variables: {}, nodes: [], edges: [] },
          prompts: {},
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
