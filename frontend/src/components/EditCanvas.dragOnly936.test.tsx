import { describe, it, expect, beforeEach, vi } from "vitest";
import { render, fireEvent, within } from "@testing-library/react";
import EditCanvas from "./EditCanvas";
import { useEditStore, type OpenPipeline } from "../stores/editStore";
import type { PipelineDef } from "../types";
import { TooltipProvider } from "./ui/tooltip";

// #936: an edge is born from a DRAG only. xyflow's click-to-connect (on by
// default in v12) turned "click a rim, then click another node" into a wire.
// This suite renders the REAL <ReactFlow> — the other EditCanvas suites stub it —
// because the gesture under test lives inside xyflow's handles.

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

vi.mock("../api", () => ({
  fetchAgentProfiles: vi.fn().mockResolvedValue({ profiles: [] }),
  fetchSkillBank: vi.fn().mockResolvedValue({ skills: [], folders: [], root_path: "" }),
  fetchProjects: vi.fn().mockResolvedValue([]),
  fetchLibrary: vi.fn().mockResolvedValue([]),
  fetchLibraryPipelines: vi.fn().mockResolvedValue([]),
  saveLibraryPipeline: vi.fn().mockResolvedValue({ id: "p", scope: "repo" }),
  deleteLibraryPipeline: vi.fn().mockResolvedValue(undefined),
  saveToLibrary: vi.fn().mockResolvedValue({}),
  deleteFromLibrary: vi.fn().mockResolvedValue(undefined),
}));

const PIPELINE: PipelineDef = {
  name: "p1",
  version: "1.0",
  variables: {},
  nodes: [
    {
      id: "writer",
      name: "Writer",
      type: "agent",
      interactive: false,
      inputs: [],
      outputs: [{ name: "draft", repeated: false }],
      view: { x: 0, y: 0 },
    },
    {
      id: "reviewer",
      name: "Reviewer",
      type: "agent",
      interactive: false,
      inputs: [],
      outputs: [{ name: "review", repeated: false }],
      view: { x: 400, y: 0 },
    },
  ],
  edges: [],
};

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
  useEditStore.setState({
    openTabs: [tab],
    activeTabId: "p1",
    selection: { kind: "none", id: null },
  });
}

function renderCanvas() {
  return render(
    <TooltipProvider>
      <EditCanvas
        libraryEntries={[]}
        libraryPipelines={[]}
        onLibraryDelete={() => {}}
        onLibraryPipelinesChanged={() => {}}
      />
    </TooltipProvider>,
  );
}

/** The rendered xyflow node wrapper of a pipeline node. */
function nodeEl(id: string): HTMLElement {
  const el = document.querySelector<HTMLElement>(`.react-flow__node[data-id="${id}"]`);
  if (!el) throw new Error(`node ${id} not rendered`);
  return el;
}

/** The body-covering target handle a drop lands on (#168). */
function bodyTarget(id: string): HTMLElement {
  const el = nodeEl(id).querySelector<HTMLElement>(".react-flow__handle.target");
  if (!el) throw new Error(`node ${id} has no target handle`);
  return el;
}

const edgeCount = () => useEditStore.getState().openTabs[0].pipeline.edges.length;

beforeEach(() => {
  localStorage.clear();
  seedTab();
});

describe("#936 — an edge is created by a drag only", () => {
  it("clicking a rim, then another node, adds no edge", () => {
    renderCanvas();

    fireEvent.click(within(nodeEl("writer")).getByTestId("rim-source-right"));
    fireEvent.click(bodyTarget("reviewer"));

    expect(edgeCount()).toBe(0);
  });

  it("a single click on a rim selects the node, like a click on the card", () => {
    renderCanvas();

    fireEvent.click(within(nodeEl("reviewer")).getByTestId("rim-source-top"));

    expect(useEditStore.getState().selection).toEqual({ kind: "node", id: "reviewer" });
  });
});
