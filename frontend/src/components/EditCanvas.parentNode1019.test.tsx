import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { render, screen, within, act } from "@testing-library/react";
import type { Node } from "@xyflow/react";
import EditCanvas, { EditNode } from "./EditCanvas";
import { useEditStore, type OpenPipeline } from "../stores/editStore";
import type { RunChildEntry, RunChildrenResponse } from "../lib/orchestration";
import type { NodeDef, RunState } from "../types";
import { TooltipProvider } from "./ui/tooltip";

// #1019 / ADR-0064 — the child-count pastilles on a run-view card follow the
// node's REAL child runs (any parent node, agent or script, toggle or not). A
// card without a child is unchanged — even with the Orchestrator toggle on.

globalThis.ResizeObserver = class {
  observe() {}
  unobserve() {}
  disconnect() {}
};

// Stub <ReactFlow> to a passthrough that renders the real `EditNode` card of
// every `edit` node it is handed — the derivation under test feeds those cards.
vi.mock("@xyflow/react", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@xyflow/react")>();
  return {
    ...actual,
    ReactFlow: (props: { nodes?: Node[]; children?: React.ReactNode }) => (
      <div data-testid="reactflow-stub">
        {(props.nodes ?? [])
          .filter((n) => n.type === "edit")
          .map((n) => (
            <div key={n.id} data-testid={`card-${n.id}`}>
              <EditNode
                {...({
                  id: n.id,
                  data: n.data,
                  type: "edit",
                  selected: false,
                  isConnectable: false,
                  zIndex: 0,
                  positionAbsoluteX: 0,
                  positionAbsoluteY: 0,
                  dragging: false,
                  deletable: false,
                  selectable: true,
                  width: 160,
                  height: 80,
                } as unknown as Parameters<typeof EditNode>[0])}
              />
            </div>
          ))}
        {props.children}
      </div>
    ),
  };
});

vi.mock("../api", () => ({
  fetchAgentProfiles: vi.fn().mockResolvedValue({ profiles: [] }),
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

const work = (id: string, over: Partial<NodeDef> = {}): NodeDef =>
  ({
    id,
    name: id,
    type: "agent",
    interactive: false,
    inputs: [],
    outputs: [{ name: "out", repeated: false, side: "right" }],
    ...over,
  }) as NodeDef;

// dispatch: agent, no toggle, 3 children · fanout: script, 2 children ·
// orchestrate: toggle on, no child · grill: neither.
const NODES: NodeDef[] = [
  work("dispatch"),
  work("fanout", { type: "script" }),
  work("orchestrate", { orchestrator: true }),
  work("grill"),
];

const PIPELINE = { name: "P", version: "1.0", variables: {}, nodes: NODES, edges: [] };

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

function runState(): RunState {
  return {
    run_id: "r1",
    status: "running",
    pipeline_name: "P",
    input: null,
    started_at: null,
    completed_at: null,
    nodes: {},
    edges: [],
    node_defs: NODES.map((n) => ({
      id: n.id,
      name: n.name,
      node_type: n.type,
      orchestrator: n.orchestrator,
      view_x: null,
      view_y: null,
      inputs: [],
      outputs: [],
    })),
    start_node: null,
    end_node: null,
    merge_resolver: null,
  };
}

const child = (id: string, over: Partial<RunChildEntry> = {}): RunChildEntry => ({
  run_id: id,
  pipeline_name: "ticket-impl",
  name: id,
  status: "running",
  ...over,
});

const CHILDREN: RunChildrenResponse = {
  run_id: "r1",
  nodes: [
    { node_id: "dispatch", children: [child("a", { status: "completed" }), child("b", { status: "completed" }), child("c")] },
    { node_id: "fanout", children: [child("d", { status: "completed" }), child("e", { status: "failed" })] },
  ],
};

const fetchMock = vi.fn(async (url: string) =>
  url === "/runs/r1/children"
    ? new Response(JSON.stringify(CHILDREN), { status: 200 })
    : new Response("not found", { status: 404 }),
);

beforeEach(() => {
  vi.clearAllMocks();
  localStorage.clear();
  vi.stubGlobal("fetch", fetchMock);
  useEditStore.setState({
    openTabs: [runTab()],
    activeTabId: "__run__r1",
    selection: { kind: "none", id: null },
  });
});

afterEach(() => {
  vi.unstubAllGlobals();
  useEditStore.setState({ selection: { kind: "none", id: null } });
});

async function renderCanvas() {
  render(
    <TooltipProvider>
      <EditCanvas
        libraryEntries={[]}
        libraryPipelines={[]}
        onLibraryDelete={() => {}}
        onLibraryPipelinesChanged={() => {}}
        runState={runState()}
      />
    </TooltipProvider>,
  );
  await screen.findAllByTestId("node-child-pills-row");
  await act(async () => {});
}

describe("Run-view card pastilles for any parent node (#1019)", () => {
  it("an agent node WITHOUT the toggle and with children carries the counters", async () => {
    await renderCanvas();
    const card = screen.getByTestId("card-dispatch");
    expect(within(card).getByTestId("node-child-pills-finished")).toHaveTextContent("2");
    expect(within(card).getByTestId("node-child-pills-running")).toHaveTextContent("1");
  });

  it("a script node with children carries them too", async () => {
    await renderCanvas();
    const card = screen.getByTestId("card-fanout");
    expect(within(card).getByTestId("node-child-pills-finished")).toHaveTextContent("1");
    expect(within(card).getByTestId("node-child-pills-failed")).toHaveTextContent("1");
  });

  it("a node without a child keeps its card unchanged — toggle on or not", async () => {
    await renderCanvas();
    expect(within(screen.getByTestId("card-grill")).queryByTestId("node-child-pills-row")).toBeNull();
    expect(within(screen.getByTestId("card-orchestrate")).queryByTestId("node-child-pills-row")).toBeNull();
  });
});
