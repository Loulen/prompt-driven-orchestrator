import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import EditCanvas from "./EditCanvas";
import { useEditStore, type OpenPipeline } from "../stores/editStore";
import type { RunState, RunStatus } from "../types";
import { TooltipProvider } from "./ui/tooltip";

// jsdom has no ResizeObserver; ReactFlow's container measurement needs it
// (mirrors EditCanvas.banner225.test.tsx).
globalThis.ResizeObserver = class {
  observe() {}
  unobserve() {}
  disconnect() {}
};

// Stub <ReactFlow> to a passthrough <div> that (a) reflects the boolean edit
// props as data-attributes so we can assert drag/connect are off, and (b)
// exposes `onNodeClick` via a button so we can prove selection still works on a
// read-only canvas. The toolbar + star are rendered OUTSIDE <ReactFlow>, so
// collapsing the canvas body does not hide what's under test.
vi.mock("@xyflow/react", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@xyflow/react")>();
  return {
    ...actual,
    ReactFlow: (props: {
      children?: React.ReactNode;
      nodesDraggable?: boolean;
      nodesConnectable?: boolean;
      onNodeClick?: (e: unknown, node: unknown) => void;
    }) => (
      <div
        data-testid="reactflow-stub"
        data-draggable={String(props.nodesDraggable)}
        data-connectable={String(props.nodesConnectable)}
      >
        <button
          data-testid="fire-node-click"
          onClick={() => props.onNodeClick?.({}, { id: "worker", type: "edit" })}
        >
          fire
        </button>
        {props.children}
      </div>
    ),
  };
});

// EditCanvas (via usePipelineLibraryState) imports the API client; stub the
// network so nothing fetches on mount.
vi.mock("../api", () => ({
  fetchAgentProfiles: vi.fn().mockResolvedValue({ profiles: [] }),
  // #669: the skills selector's reads (bank + inherited tiers), empty by default.
  fetchSkillBank: vi.fn().mockResolvedValue({ skills: [], folders: [], root_path: "" }),
  fetchProjects: vi.fn().mockResolvedValue([]),
  fetchLibrary: vi.fn().mockResolvedValue([]),
  fetchLibraryPipelines: vi.fn().mockResolvedValue([]),
  saveLibraryPipeline: vi.fn().mockResolvedValue({ id: "my-pipeline", scope: "repo" }),
  deleteLibraryPipeline: vi.fn().mockResolvedValue(undefined),
  saveToLibrary: vi.fn().mockResolvedValue({}),
  deleteFromLibrary: vi.fn().mockResolvedValue(undefined),
  fetchRunPipeline: vi.fn().mockRejectedValue(new Error("offline")),
}));

const PIPELINE = {
  name: "My Pipeline",
  version: "1.0",
  variables: {},
  nodes: [
    {
      id: "start",
      name: "Start",
      type: "start" as const,
      interactive: false,
      inputs: [],
      outputs: [{ name: "user_prompt", repeated: false, side: "right" as const }],
    },
    {
      id: "worker",
      name: "Worker",
      type: "agent" as const,
      interactive: false,
      inputs: [{ name: "task", repeated: false, side: "left" as const }],
      outputs: [{ name: "result", repeated: false, side: "right" as const }],
    },
    {
      id: "end",
      name: "End",
      type: "end" as const,
      interactive: false,
      inputs: [{ name: "result", repeated: false, side: "left" as const }],
      outputs: [],
    },
  ],
  edges: [
    { source: { node: "start", port: "user_prompt" }, target: { node: "worker", port: "task" } },
    { source: { node: "worker", port: "result" }, target: { node: "end", port: "result" } },
  ],
};

// A run tab (`__run__r1`) — shaped as `openRunPipeline` produces it.
function runTab(): OpenPipeline {
  return {
    id: "__run__r1",
    scope: "run",
    pipeline: PIPELINE,
    prompts: {},
    diagnostics: [],
    dirty: false,
    externalDirty: false,
    runId: "r1",
    libraryId: null,
    libraryScope: null,
  };
}

// Minimal RunState matching the tab's runId, with a controllable status.
function runState(status: RunStatus): RunState {
  return {
    run_id: "r1",
    status,
    pipeline_name: "My Pipeline",
    input: null,
    started_at: null,
    completed_at: null,
    nodes: {},
    edges: [],
    node_defs: [],
    start_node: null,
    end_node: null,
    merge_resolver: null,
  };
}

function seedRunTab() {
  useEditStore.setState({
    openTabs: [runTab()],
    activeTabId: "__run__r1",
    selection: { kind: "none", id: null },
  });
}

function renderCanvas(status: RunStatus) {
  return render(
    <TooltipProvider>
      <EditCanvas
        libraryEntries={[]}
        libraryPipelines={[]}
        onLibraryDelete={() => {}}
        onLibraryPipelinesChanged={() => {}}
        runState={runState(status)}
      />
    </TooltipProvider>,
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  localStorage.clear();
  seedRunTab();
});

afterEach(() => {
  useEditStore.setState({ selection: { kind: "none", id: null } });
});

describe("#315 — EditCanvas is read-only for an archived run", () => {
  it("hides every add/edit toolbar control on an archived run", () => {
    renderCanvas("archived");

    // The add-node dropdown and script buttons are gone.
    expect(screen.queryByTestId("toolbar-add")).toBeNull();
    expect(screen.queryByTestId("toolbar-script")).toBeNull();
  });

  it("turns drag and connect off on an archived run", () => {
    renderCanvas("archived");

    const stub = screen.getByTestId("reactflow-stub");
    expect(stub.getAttribute("data-draggable")).toBe("false");
    expect(stub.getAttribute("data-connectable")).toBe("false");
  });

  it("keeps node selection working on an archived run (inspection is the point)", () => {
    renderCanvas("archived");

    fireEvent.click(screen.getByTestId("fire-node-click"));

    expect(useEditStore.getState().selection).toEqual({ kind: "node", id: "worker" });
  });

  it("an archived run offers no Edit control (read-only for good, ADR-0080)", () => {
    renderCanvas("archived");
    expect(screen.queryByTestId("toolbar-edit")).toBeNull();
    expect(screen.queryByTestId("toolbar-finish-editing")).toBeNull();
  });
});

describe("ADR-0080 — a run tab opens in « pilotage », « Edit for this run » unlocks it", () => {
  it("locks a live run's canvas: no drag, no connect, no authoring control", () => {
    renderCanvas("running");
    const stub = screen.getByTestId("reactflow-stub");
    expect(stub.getAttribute("data-draggable")).toBe("false");
    expect(stub.getAttribute("data-connectable")).toBe("false");
    for (const id of ["toolbar-add", "toolbar-library", "toolbar-script", "toolbar-undo", "toolbar-redo"]) {
      expect(screen.queryByTestId(id), id).toBeNull();
    }
    expect(screen.getByTestId("toolbar-edit")).toBeInTheDocument();
  });

  it("keeps the run controls of a finished run while locked", () => {
    renderCanvas("completed");
    expect(screen.getByTestId("reactflow-stub").getAttribute("data-draggable")).toBe("false");
    expect(screen.getByTestId("toolbar-reopen")).toBeInTheDocument();
    expect(screen.getByTestId("toolbar-retry-all")).toBeInTheDocument();
    expect(screen.getByTestId("toolbar-open-shell")).toBeInTheDocument();
    expect(screen.getByTestId("toolbar-review")).toBeInTheDocument();
  });

  it("is fully editable once « Edit for this run » is chosen (ADR-0007 hot editing)", () => {
    useEditStore.getState().startRunEditing("__run__r1");
    renderCanvas("completed");
    expect(screen.getByTestId("toolbar-add")).toBeInTheDocument();
    expect(screen.getByTestId("toolbar-script")).toBeInTheDocument();
    const stub = screen.getByTestId("reactflow-stub");
    expect(stub.getAttribute("data-draggable")).toBe("true");
    expect(stub.getAttribute("data-connectable")).toBe("true");
    expect(screen.getByTestId("toolbar-finish-editing")).toBeInTheDocument();
  });

  it("« Edit source pipeline » opens the run's source pipeline tab", async () => {
    const openPipeline = vi.fn().mockResolvedValue(undefined);
    useEditStore.setState({ openPipeline });
    renderCanvas("running");
    fireEvent.pointerDown(screen.getByTestId("toolbar-edit"));
    fireEvent.click(screen.getByTestId("toolbar-edit"));
    fireEvent.click(await screen.findByTestId("toolbar-edit-source"));
    expect(openPipeline).toHaveBeenCalledWith("My Pipeline");
  });

  it("« Finish editing » locks again at once when nothing is unsaved", () => {
    useEditStore.getState().startRunEditing("__run__r1");
    renderCanvas("running");
    fireEvent.click(screen.getByTestId("toolbar-finish-editing"));
    expect(screen.queryByTestId("finish-editing-modal")).toBeNull();
    expect(screen.getByTestId("toolbar-edit")).toBeInTheDocument();
    expect(screen.getByTestId("reactflow-stub").getAttribute("data-draggable")).toBe("false");
  });

  it("« Finish editing » with unsaved edits asks first, and Keep editing keeps them", () => {
    useEditStore.getState().startRunEditing("__run__r1");
    useEditStore.getState().updatePrompt("worker", "draft");
    renderCanvas("running");
    fireEvent.click(screen.getByTestId("toolbar-finish-editing"));
    expect(screen.getByTestId("finish-editing-modal")).toBeInTheDocument();
    fireEvent.click(screen.getByTestId("finish-editing-cancel"));
    expect(screen.queryByTestId("finish-editing-modal")).toBeNull();
    expect(useEditStore.getState().openTabs[0].prompts.worker).toBe("draft");
    expect(screen.getByTestId("toolbar-finish-editing")).toBeInTheDocument();
  });
});
