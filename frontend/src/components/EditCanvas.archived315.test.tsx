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
  fetchLibrary: vi.fn().mockResolvedValue([]),
  fetchLibraryPipelines: vi.fn().mockResolvedValue([]),
  saveLibraryPipeline: vi.fn().mockResolvedValue({ id: "my-pipeline", scope: "repo" }),
  deleteLibraryPipeline: vi.fn().mockResolvedValue(undefined),
  saveToLibrary: vi.fn().mockResolvedValue({}),
  deleteFromLibrary: vi.fn().mockResolvedValue(undefined),
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
      type: "doc-only" as const,
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

    // The add-node dropdown, merge, and script buttons are gone.
    expect(screen.queryByTestId("toolbar-add")).toBeNull();
    expect(screen.queryByTestId("toolbar-merge")).toBeNull();
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

  it("stays fully editable for a non-archived (completed) run — read-only is archive-only", () => {
    renderCanvas("completed");

    // Editing affordances present …
    expect(screen.getByTestId("toolbar-add")).toBeInTheDocument();
    expect(screen.getByTestId("toolbar-merge")).toBeInTheDocument();
    // … and drag/connect are on (ADR-0007 editing-during-run must not regress).
    const stub = screen.getByTestId("reactflow-stub");
    expect(stub.getAttribute("data-draggable")).toBe("true");
    expect(stub.getAttribute("data-connectable")).toBe("true");
  });
});
