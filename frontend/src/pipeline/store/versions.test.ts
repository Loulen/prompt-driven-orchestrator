import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useEditStore } from "./editStore";
import { INITIAL_DRAFT_STATE } from "./draftSession";
import { isTabLocked, versionTabId, type OpenPipeline } from "./editState";
import { takeDraftGesture } from "./draftGesture";

vi.mock("../api/pipelines", () => ({
  savePipeline: vi.fn(),
  fetchPipeline: vi.fn(),
  fetchPipelines: vi.fn().mockResolvedValue([]),
}));
vi.mock("../api/versions", () => ({ fetchPipelineVersion: vi.fn(), fetchPipelineVersions: vi.fn() }));
const { savePipeline } = await import("../api/pipelines");
const { fetchPipelineVersion } = await import("../api/versions");

const v1 = {
  number: 1,
  title: "v1",
  author: null,
  saved_at: "2026-10-14T10:00:00Z",
  external: false,
  yaml: "name: flow\n",
  pipeline: { name: "flow", variables: {}, nodes: [], edges: [] },
  prompts: { a: "Old." },
};

function tab(patch: Partial<OpenPipeline> = {}): OpenPipeline {
  return {
    id: "flow",
    scope: "instance",
    pipeline: { name: "flow", variables: {}, nodes: [{ id: "a", name: "A", type: "agent", inputs: [], outputs: [], interactive: false }], edges: [] },
    prompts: { a: "New." },
    diagnostics: [],
    dirty: true,
    externalDirty: false,
    ...patch,
  } as OpenPipeline;
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(fetchPipelineVersion).mockResolvedValue(v1);
});

afterEach(() => {
  useEditStore.setState({ openTabs: [], activeTabId: null, saveVersionFor: null });
  takeDraftGesture("flow");
});

describe("versions in the editor's store (ADR-0088)", () => {
  it("opens the « Save… » window for a shared draft, and saves a tab without one", () => {
    useEditStore.setState({ openTabs: [tab({ draft: { ...INITIAL_DRAFT_STATE, synced: true, canWrite: true, changes: 1 } })] });
    useEditStore.getState().requestSave("flow");
    expect(useEditStore.getState().saveVersionFor).toBe("flow");
    expect(savePipeline).not.toHaveBeenCalled();
    useEditStore.getState().closeSaveVersion();
    expect(useEditStore.getState().saveVersionFor).toBeNull();

    vi.mocked(savePipeline).mockResolvedValueOnce({ ok: true });
    useEditStore.setState({ openTabs: [tab()] });
    useEditStore.getState().requestSave("flow");
    expect(useEditStore.getState().saveVersionFor).toBeNull();
    expect(savePipeline).toHaveBeenCalledTimes(1);
  });

  it("opens a version in a read-only tab of its own", async () => {
    await useEditStore.getState().openPipelineVersion("flow", 1);
    const state = useEditStore.getState();
    const opened = state.openTabs.find((t) => t.id === versionTabId("flow", 1));
    expect(opened?.version).toMatchObject({ pipelineId: "flow", number: 1, title: "v1" });
    expect(opened?.draft).toBeUndefined();
    expect(isTabLocked(opened)).toBe(true);
    expect(state.activeTabId).toBe(versionTabId("flow", 1));
    // Nothing to save there.
    await expect(useEditStore.getState().save(versionTabId("flow", 1))).resolves.toBeUndefined();
    expect(savePipeline).not.toHaveBeenCalled();
  });

  it("restores a version into the draft as one change of mine", async () => {
    useEditStore.setState({
      openTabs: [tab({ draft: { ...INITIAL_DRAFT_STATE, synced: true, canWrite: true, changes: 1 } })],
      activeTabId: "flow",
    });
    await useEditStore.getState().restoreVersion("flow", 1);
    const restored = useEditStore.getState().openTabs[0];
    expect(restored.pipeline).toEqual(v1.pipeline);
    expect(restored.pipeline.name).toBe("flow");
    expect(restored.prompts).toEqual({ a: "Old." });
    expect(takeDraftGesture("flow")).toEqual({ tabId: "flow", coalesceKey: null, track: true });
    expect(savePipeline).not.toHaveBeenCalled();
  });

  it("keeps the pipeline's name when the version had another one", async () => {
    vi.mocked(fetchPipelineVersion).mockResolvedValue({ ...v1, pipeline: { ...v1.pipeline, name: "before-rename" } });
    useEditStore.setState({
      openTabs: [tab({ draft: { ...INITIAL_DRAFT_STATE, synced: true, canWrite: true } })],
      activeTabId: "flow",
    });
    await useEditStore.getState().restoreVersion("flow", 1);
    expect(useEditStore.getState().openTabs[0].pipeline.name).toBe("flow");
  });

  it("restores nothing into a draft it may only read", async () => {
    vi.useFakeTimers();
    useEditStore.setState({
      openTabs: [tab({ draft: { ...INITIAL_DRAFT_STATE, synced: true, canWrite: false } })],
      activeTabId: "flow",
    });
    const restoring = useEditStore.getState().restoreVersion("flow", 1);
    const refused = expect(restoring).rejects.toThrow("not open for writing");
    await vi.advanceTimersByTimeAsync(6_000);
    await refused;
    expect(useEditStore.getState().openTabs[0].prompts).toEqual({ a: "New." });
    vi.useRealTimers();
  });
});
