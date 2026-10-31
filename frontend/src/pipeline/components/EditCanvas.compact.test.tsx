import { describe, it, expect, beforeEach, vi } from "vitest";
import { act, render, screen } from "@testing-library/react";
import EditCanvas from "./EditCanvas";
import { useEditStore } from "../store/editStore";
import { type OpenPipeline } from "../store/editState";
import { resetCompactNodes, setCompactNodes } from "../../hooks/useCompactNodes";
import type { RunState } from "../../shared/generated/RunState";
import type { EditorPipelineDef } from "../model/pipeline";

globalThis.ResizeObserver = class {
  observe() {}
  unobserve() {}
  disconnect() {}
};

// <ReactFlow> as a list of what it would draw: each node's position and card
// size, each edge's anchors and waypoints.
vi.mock("@xyflow/react", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@xyflow/react")>();
  return {
    ...actual,
    ReactFlow: (props: {
      children?: React.ReactNode;
      nodes: { id: string; position: { x: number; y: number }; data: { compact?: boolean } }[];
      edges: { id: string; data: { sourceAnchor?: unknown; waypoints?: unknown } }[];
    }) => (
      <div data-testid="reactflow-stub">
        {props.nodes.map((n) => (
          <div key={n.id} data-testid={`rf-node-${n.id}`} data-x={n.position.x} data-y={n.position.y} data-compact={n.data.compact ? "" : undefined} />
        ))}
        {props.edges.map((e) => (
          <div key={e.id} data-testid={`rf-edge-${e.id}`} data-route={JSON.stringify([e.data.sourceAnchor ?? null, e.data.waypoints ?? null])} />
        ))}
        {props.children}
      </div>
    ),
  };
});

vi.mock("../../skill/api/bank", () => ({
  fetchSkillBank: vi.fn().mockResolvedValue({ skills: [], folders: [], root_path: "" }),
}));

// EditCanvas (via usePipelineLibraryState) imports the API client; stub the
// network so nothing fetches on mount.
vi.mock("../../settings/api/settings", () => ({
  fetchAgentProfiles: vi.fn().mockResolvedValue({ profiles: [] }),
  fetchLibrary: vi.fn().mockResolvedValue([]),
  fetchLibraryPipelines: vi.fn().mockResolvedValue([]),
  saveLibraryPipeline: vi.fn().mockResolvedValue({ id: "my-pipeline", scope: "repo" }),
  deleteLibraryPipeline: vi.fn().mockResolvedValue(undefined),
  saveToLibrary: vi.fn().mockResolvedValue({}),
  deleteFromLibrary: vi.fn().mockResolvedValue(undefined),
}));
// The agentic profiles' API answers with the mock of `../../settings/api/settings` too.
vi.mock("../../settings/api/agentProfiles", async () => await import("../../settings/api/settings"));
// The pipeline context's API answers with the mock of `../api` above.
vi.mock("../api/library", async (importOriginal) => ({
  ...(await importOriginal<object>()),
  ...(await import("../../settings/api/settings")),
}));
vi.mock("../api/libraryPipelines", async (importOriginal) => ({
  ...(await importOriginal<object>()),
  ...(await import("../../settings/api/settings")),
}));

vi.mock("../../run/api/runPipeline", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../run/api/runPipeline")>()),
  fetchRunPipeline: vi.fn().mockRejectedValue(new Error("offline")),
  fetchRunPipelineOverwritePreview: vi.fn().mockRejectedValue(new Error("offline")),
}));

vi.mock("../../workspace/api/projects", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../workspace/api/projects")>()),
  fetchProjects: vi.fn().mockResolvedValue([]),
}));

const node = (id: string, x: number, y: number, type: "agent" | "start" | "end" = "agent") => ({
  id,
  name: id,
  type,
  interactive: false,
  inputs: [],
  outputs: type === "end" ? [] : [{ name: "out" }],
  view: { x, y },
});

const PIPELINE: EditorPipelineDef = {
  name: "p",
  version: "1.0",
  variables: {},
  nodes: [node("start", 80, 0, "start"), node("a", 0, 100), node("b", 0, 240), node("c", 300, 240), node("end", 80, 400, "end")],
  edges: [
    { source: { node: "start", port: "out" }, target: { node: "a", port: "in" }, target_side: "top" },
    {
      source: { node: "a", port: "out" },
      target: { node: "c", port: "in" },
      mode: "manual",
      waypoints: [
        { x: 240, y: 133 },
        { x: 240, y: 273 },
      ],
    },
    { source: { node: "b", port: "out" }, target: { node: "end", port: "result" }, source_anchor: { side: "bottom" }, target_side: "top" },
  ],
};

function tab(runId: string | null, runEditing = false): OpenPipeline {
  return {
    id: runId ? `__run__${runId}` : "p",
    scope: runId ? "run" : "repo",
    pipeline: PIPELINE,
    prompts: {},
    diagnostics: [],
    dirty: false,
    externalDirty: false,
    ...(runId ? { runId, runEditing } : {}),
    libraryId: null,
    libraryScope: null,
  } as OpenPipeline;
}

const RUN = {
  loop_states: {},
  foreach_states: {},
  collection_states: {},
  switch_states: {},
  sandbox: "off",
  sessions_spawned: 0,
  run_id: "r1",
  status: "running",
  pipeline_name: "p",
  input: null,
  started_at: null,
  completed_at: null,
  nodes: {},
  edges: [],
  node_defs: [],
} as unknown as RunState;

function draw(t: OpenPipeline) {
  useEditStore.setState({ openTabs: [t], activeTabId: t.id, selection: { kind: "none", id: null } });
  return render(<EditCanvas libraryEntries={[]} onLibraryDelete={() => {}} runState={t.runId ? RUN : null} />);
}

const at = (id: string) => {
  const el = screen.getByTestId(`rf-node-${id}`);
  return { x: Number(el.dataset.x), y: Number(el.dataset.y), compact: el.hasAttribute("data-compact") };
};

beforeEach(() => {
  vi.clearAllMocks();
  localStorage.clear();
  resetCompactNodes();
});

describe("Compact nodes on the canvas", () => {
  it("draws a followed Run's cards on one line and lifts what lies below, without writing the pipeline", () => {
    setCompactNodes(true);
    const before = JSON.stringify(PIPELINE);
    draw(tab("r1"));
    expect(at("a")).toEqual({ x: 20, y: 100, compact: true });
    expect(at("b")).toEqual({ x: 20, y: 205, compact: true });
    expect(at("c")).toEqual({ x: 320, y: 205, compact: true });
    expect(at("start")).toEqual({ x: 80, y: 0, compact: false });
    expect(at("end")).toEqual({ x: 80, y: 330, compact: false });
    // The side port folds onto the first line, and the manual route follows it.
    expect(JSON.parse(screen.getByTestId("rf-edge-e-1").dataset.route!)).toEqual([
      { side: "right", offset: 15 },
      [
        { x: 240, y: 115 },
        { x: 240, y: 220 },
      ],
    ]);
    const stored = useEditStore.getState().openTabs[0];
    expect(stored.pipeline).toBe(PIPELINE);
    expect(JSON.stringify(stored.pipeline)).toBe(before);
    expect(stored.dirty).toBe(false);
  });

  it("goes back to the detailed layout exactly when turned off", () => {
    setCompactNodes(true);
    draw(tab("r1"));
    act(() => setCompactNodes(false));
    expect(at("a")).toEqual({ x: 0, y: 100, compact: false });
    expect(at("b")).toEqual({ x: 0, y: 240, compact: false });
    expect(at("end")).toEqual({ x: 80, y: 400, compact: false });
    expect(JSON.parse(screen.getByTestId("rf-edge-e-1").dataset.route!)).toEqual([null, PIPELINE.edges[1].waypoints]);
  });

  it("keeps the detailed cards in the pipeline editor", () => {
    setCompactNodes(true);
    draw(tab(null));
    expect(at("b")).toEqual({ x: 0, y: 240, compact: false });
  });

  it("keeps the detailed cards on a Run edited for itself, where a drag writes", () => {
    setCompactNodes(true);
    draw(tab("r1", true));
    expect(at("b")).toEqual({ x: 0, y: 240, compact: false });
  });
});
