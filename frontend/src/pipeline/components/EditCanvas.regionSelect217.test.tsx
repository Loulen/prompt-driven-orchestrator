import { describe, it, expect, beforeEach, vi } from "vitest";
import { render, fireEvent, act } from "@testing-library/react";
import EditCanvas from "./EditCanvas";
import { useEditStore } from "../store/editStore";
import { type OpenPipeline } from "../store/editState";
import type { EditorPipelineDef } from "../model/pipeline";
import type { RunState } from "../../shared/generated/RunState";

// CORE-217: opening a loop region's inspector — its corner or `raise cap` on
// the canvas, the « budget reached » banner of Run info — hands it the pane:
// the card selected before loses its outline (xyflow keeps its own selection,
// which only a click on another node or on the pane clears) and Pipeline info
// gives way. The REAL <ReactFlow>, because that selection lives inside xyflow.

// jsdom has no ResizeObserver; ReactFlow's container measurement needs it.
globalThis.ResizeObserver = class {
  observe() {}
  unobserve() {}
  disconnect() {}
};
// jsdom has no hit-testing either. xyflow's click-to-connect asks what lies under
// the pointer and, finding nothing, falls back to the clicked handle — so with
// this stub a click-click WOULD wire the two handles if click-to-connect were on.
document.elementFromPoint = () => null;

vi.mock("../../skill/api/bank", () => ({
  fetchSkillBank: vi.fn().mockResolvedValue({ skills: [], folders: [], root_path: "" }),
}));
vi.mock("../../settings/api/settings", () => ({
  fetchAgentProfiles: vi.fn().mockResolvedValue({ profiles: [] }),
  fetchLibrary: vi.fn().mockResolvedValue([]),
  fetchLibraryPipelines: vi.fn().mockResolvedValue([]),
  saveLibraryPipeline: vi.fn().mockResolvedValue({ id: "p", scope: "repo" }),
  deleteLibraryPipeline: vi.fn().mockResolvedValue(undefined),
  saveToLibrary: vi.fn().mockResolvedValue({}),
  deleteFromLibrary: vi.fn().mockResolvedValue(undefined),
}));
vi.mock("../../settings/api/agentProfiles", async () => await import("../../settings/api/settings"));
vi.mock("../api/library", async (importOriginal) => ({
  ...(await importOriginal<object>()),
  ...(await import("../../settings/api/settings")),
}));
vi.mock("../api/libraryPipelines", async (importOriginal) => ({
  ...(await importOriginal<object>()),
  ...(await import("../../settings/api/settings")),
}));
vi.mock("../../workspace/api/projects", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../workspace/api/projects")>()),
  fetchProjects: vi.fn().mockResolvedValue([]),
}));

const PIPELINE: EditorPipelineDef = {
  name: "p1",
  version: "1.0",
  variables: {},
  nodes: [
    { id: "writer", name: "Writer", type: "agent", interactive: false, inputs: [], outputs: [{ name: "draft" }], view: { x: 0, y: 0 } },
    { id: "reviewer", name: "Reviewer", type: "agent", interactive: false, inputs: [], outputs: [{ name: "review" }], view: { x: 400, y: 0 } },
    { id: "ship", name: "Ship", type: "agent", interactive: false, inputs: [], outputs: [], view: { x: 800, y: 0 } },
  ],
  edges: [],
  loops: [{ id: "review_loop", kind: "bounded", members: ["writer", "reviewer"], max_iter: 3 }],
} as EditorPipelineDef;

function seedTab() {
  const tab: OpenPipeline = {
    id: "p1",
    scope: "repo",
    pipeline: structuredClone(PIPELINE),
    prompts: {},
    diagnostics: [],
    dirty: false,
    externalDirty: false,
    libraryId: null,
    libraryScope: null,
  };
  useEditStore.setState({ openTabs: [tab], activeTabId: "p1", selection: { kind: "none", id: null } });
}

/** The rendered xyflow node wrapper of a pipeline node. */
function nodeEl(id: string): HTMLElement {
  const el = document.querySelector<HTMLElement>(`.react-flow__node[data-id="${id}"]`);
  if (!el) throw new Error(`node ${id} not rendered`);
  return el;
}

const REGION = { kind: "region", id: null, regionId: "review_loop" } as const;

beforeEach(() => {
  localStorage.clear();
  seedTab();
});

describe("CORE-217 — a loop region's inspector takes the pane", () => {
  it("the region's corner deselects the card selected before and closes Pipeline info", () => {
    const onCloseInfo = vi.fn();
    render(<EditCanvas libraryEntries={[]} onLibraryDelete={() => {}} onCloseInfo={onCloseInfo} />);
    fireEvent.click(nodeEl("ship"));
    expect(nodeEl("ship")).toHaveClass("selected");
    onCloseInfo.mockClear();

    fireEvent.click(document.querySelector<HTMLElement>('[data-testid="loop-region-header"]')!);

    expect(useEditStore.getState().selection).toEqual(REGION);
    expect(nodeEl("ship")).not.toHaveClass("selected");
    expect(onCloseInfo).toHaveBeenCalled();
  });

  it("a region selected from outside the canvas deselects the card too", () => {
    render(<EditCanvas libraryEntries={[]} onLibraryDelete={() => {}} />);
    fireEvent.click(nodeEl("ship"));
    expect(nodeEl("ship")).toHaveClass("selected");

    act(() => useEditStore.getState().setSelection(REGION));

    expect(nodeEl("ship")).not.toHaveClass("selected");
  });

  it("raise cap on a Run parked « budget reached » opens the inspector the same way", () => {
    useEditStore.setState({
      openTabs: [
        {
          id: "__run__r1",
          scope: "run",
          pipeline: structuredClone(PIPELINE),
          prompts: {},
          diagnostics: [],
          dirty: false,
          externalDirty: false,
          runId: "r1",
          libraryId: null,
          libraryScope: null,
        },
      ],
      activeTabId: "__run__r1",
    });
    const run = {
      run_id: "r1",
      pipeline_name: "p1",
      status: "awaiting_user",
      awaiting_reason_code: "region_budget_exhausted",
      awaiting_region_id: "review_loop",
      input: null,
      started_at: null,
      completed_at: null,
      nodes: {},
      edges: [],
      node_defs: [],
      loop_states: {},
      foreach_states: {},
      collection_states: {},
      switch_states: {},
      sandbox: "off",
      sessions_spawned: 0,
    } as unknown as RunState;
    const onCloseInfo = vi.fn();
    render(<EditCanvas libraryEntries={[]} onLibraryDelete={() => {}} onCloseInfo={onCloseInfo} runState={run} />);
    fireEvent.click(nodeEl("ship"));
    onCloseInfo.mockClear();

    fireEvent.click(document.querySelector<HTMLElement>('[data-testid="loop-region-raise-cap"]')!);

    // CORE-218: on the cap, focused, with the Run's tab edited for itself.
    expect(useEditStore.getState().selection).toEqual({ ...REGION, focusField: "max_cost_usd" });
    expect(useEditStore.getState().openTabs[0].runEditing).toBe(true);
    expect(nodeEl("ship")).not.toHaveClass("selected");
    expect(onCloseInfo).toHaveBeenCalled();
  });

  it("CORE-218: an open « Edit for this run » follows a cap lowered outside it (the CLI)", () => {
    useEditStore.setState({
      openTabs: [
        {
          id: "__run__r1",
          scope: "run",
          pipeline: {
            ...structuredClone(PIPELINE),
            loops: [{ id: "review_loop", kind: "bounded", members: ["writer", "reviewer"], max_iter: 3, max_cost_usd: 5 }],
          } as EditorPipelineDef,
          prompts: {},
          diagnostics: [],
          dirty: false,
          externalDirty: false,
          runId: "r1",
        },
      ],
      activeTabId: "__run__r1",
    });
    const run = (overrides: Record<string, number | null>) =>
      ({
        run_id: "r1",
        pipeline_name: "p1",
        status: "running",
        nodes: {},
        edges: [],
        node_defs: [],
        loop_states: {},
        foreach_states: {},
        collection_states: {},
        switch_states: {},
        sandbox: "off",
        sessions_spawned: 0,
        region_max_cost_overrides: overrides,
      }) as unknown as RunState;
    const { rerender } = render(<EditCanvas libraryEntries={[]} onLibraryDelete={() => {}} runState={run({ review_loop: 8 })} />);
    act(() => useEditStore.getState().startRunEditing("__run__r1", run({ review_loop: 8 })));
    expect(useEditStore.getState().openTabs[0].pipeline.loops?.[0].max_cost_usd).toBe(8);

    rerender(<EditCanvas libraryEntries={[]} onLibraryDelete={() => {}} runState={run({ review_loop: 4 })} />);

    expect(useEditStore.getState().openTabs[0].pipeline.loops?.[0].max_cost_usd).toBe(4);
    expect(useEditStore.getState().openTabs[0].dirty).toBe(false);
    expect(document.querySelector('[data-testid="loop-region-cost"]')?.textContent).toContain("$4");
  });
});
