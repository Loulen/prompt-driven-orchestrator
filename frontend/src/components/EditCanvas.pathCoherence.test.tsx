import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { act, render, screen, fireEvent } from "@testing-library/react";
import { ReactFlowProvider, type Node } from "@xyflow/react";
import EditCanvas, { EditNode } from "./EditCanvas";
import { useEditStore, type OpenPipeline } from "../stores/editStore";
import { TooltipProvider } from "./ui/tooltip";
import type { NodeStatus, NodeType, PathDefect, PipelineDef } from "../types";

// gitlab #23 / #1022 — path coherence, live in the editor (ADR-0081): the banner
// and the node marks follow every graph change without saving; the daemon's
// answer (load / Save) replaces the live rows with the same sentences.

globalThis.ResizeObserver = class {
  observe() {}
  unobserve() {}
  disconnect() {}
};

// The canvas body is stubbed; the nodes it is handed are captured so the marks
// each card carries can be asserted without a laid-out React Flow.
let canvasNodes: Node[] = [];
vi.mock("@xyflow/react", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@xyflow/react")>();
  return {
    ...actual,
    ReactFlow: ({ children, nodes }: { children?: React.ReactNode; nodes: Node[] }) => {
      canvasNodes = nodes;
      return <div data-testid="reactflow-stub">{children}</div>;
    },
  };
});

vi.mock("../api", () => ({
  fetchAgentProfiles: vi.fn().mockResolvedValue({ profiles: [] }),
  fetchSkillBank: vi.fn().mockResolvedValue({ skills: [], folders: [], root_path: "" }),
  fetchProjects: vi.fn().mockResolvedValue([]),
  fetchLibrary: vi.fn().mockResolvedValue([]),
  fetchLibraryPipelines: vi.fn().mockResolvedValue([]),
  saveLibraryPipeline: vi.fn().mockResolvedValue({ id: "p1", scope: "repo" }),
  deleteLibraryPipeline: vi.fn().mockResolvedValue(undefined),
  saveToLibrary: vi.fn().mockResolvedValue({}),
  deleteFromLibrary: vi.fn().mockResolvedValue(undefined),
}));

const NO_PATH = "Not launchable: no path from Start to End.";
const orphan = (name: string) => `Not launchable: orphan node — node "${name}" is not reachable from Start.`;
const deadEnd = (name: string) => `Not launchable: dead end — node "${name}" never leads to End.`;

const START_EDGE = {
  source: { node: "start", port: "user_prompt" },
  target: { node: "impl", port: "task" },
};

// Start → implementer → tester → End: launchable.
function pipeline(): PipelineDef {
  const work = (id: string, name: string) => ({
    id,
    name,
    type: "agent" as const,
    interactive: false,
    inputs: [],
    outputs: [{ name: "out" }],
  });
  return {
    name: "feature",
    variables: {},
    nodes: [
      { id: "start", name: "Start", type: "start", interactive: false, inputs: [], outputs: [{ name: "user_prompt" }] },
      { id: "end", name: "End", type: "end", interactive: false, inputs: [{ name: "result" }], outputs: [] },
      work("impl", "implementer"),
      work("tester", "tester"),
    ],
    edges: [
      START_EDGE,
      { source: { node: "impl", port: "out" }, target: { node: "tester", port: "code" } },
      { source: { node: "tester", port: "out" }, target: { node: "end", port: "result" } },
    ],
  };
}

function seed(patch: Partial<OpenPipeline> = {}) {
  useEditStore.setState({
    openTabs: [
      {
        id: "p1",
        scope: "repo",
        pipeline: pipeline(),
        prompts: {},
        diagnostics: [],
        pathDefects: [],
        dirty: false,
        externalDirty: false,
        libraryId: null,
        libraryScope: null,
        ...patch,
      },
    ],
    activeTabId: "p1",
    selection: { kind: "none", id: null },
  });
}

function renderCanvas(props: { coherenceTeaching?: boolean } = {}) {
  return render(
    <TooltipProvider>
      <EditCanvas libraryEntries={[]} onLibraryDelete={() => {}} {...props} />
    </TooltipProvider>,
  );
}

const marksOf = (id: string) =>
  (canvasNodes.find((n) => n.id === id)?.data as { pathMarks?: string[] } | undefined)?.pathMarks;
const toneOf = (id: string) =>
  (canvasNodes.find((n) => n.id === id)?.data as { pathMarkTone?: string } | undefined)?.pathMarkTone;
const rows = () => screen.queryAllByTestId("lint-banner-row").map((r) => r.textContent ?? "");

beforeEach(() => {
  localStorage.clear();
  canvasNodes = [];
});
afterEach(() => {
  useEditStore.setState({ selection: { kind: "none", id: null } });
});

describe("live path coherence in the editor (#1022)", () => {
  it("flags the deleted Start edge at once, without saving, and clears once it is back", () => {
    seed();
    renderCanvas();
    expect(screen.queryByTestId("lint-banner")).toBeNull();
    expect(marksOf("impl")).toBeUndefined();

    act(() => useEditStore.getState().deleteEdge(0));

    expect(useEditStore.getState().openTabs[0].dirty).toBe(true);
    expect(rows()).toEqual([
      expect.stringContaining(NO_PATH),
      expect.stringContaining(orphan("implementer")),
      expect.stringContaining(orphan("tester")),
    ]);
    // « no path » names no node: no Show; each orphan row has one.
    expect(screen.getAllByRole("button", { name: "Show" })).toHaveLength(2);
    // Start has no outgoing edge any more: « no path » is pinned on it.
    expect(marksOf("start")).toEqual(["no_path"]);
    expect(marksOf("impl")).toEqual(["orphan"]);
    expect(marksOf("tester")).toEqual(["orphan"]);
    expect(marksOf("end")).toBeUndefined();
    expect(toneOf("impl")).toBe("failed");

    act(() => useEditStore.getState().addEdge(START_EDGE));

    expect(screen.queryByTestId("lint-banner")).toBeNull();
    expect(marksOf("start")).toBeUndefined();
    expect(marksOf("impl")).toBeUndefined();
  });

  it("marks a node that never leads to End, live, and keeps « no path » off a wired Start", () => {
    seed();
    renderCanvas();
    act(() => useEditStore.getState().deleteEdge(2)); // tester → End

    expect(rows()).toEqual([
      expect.stringContaining(NO_PATH),
      expect.stringContaining(deadEnd("implementer")),
      expect.stringContaining(deadEnd("tester")),
    ]);
    expect(marksOf("start")).toBeUndefined();
    expect(marksOf("tester")).toEqual(["dead_end"]);
  });

  it("renders the daemon's answer once — never a live copy beside it", () => {
    const defects: PathDefect[] = [
      { kind: "no_path", message: NO_PATH },
      { kind: "orphan", node_id: "impl", node_name: "implementer", message: orphan("implementer") },
      { kind: "orphan", node_id: "tester", node_name: "tester", message: orphan("tester") },
    ];
    const p = pipeline();
    p.edges = p.edges.slice(1);
    seed({
      pipeline: p,
      diagnostics: ["unknown field 'x' (ignored)", ...defects.map((d) => d.message)],
      pathDefects: defects,
    });
    renderCanvas();

    expect(rows()).toEqual([
      expect.stringContaining("unknown field 'x' (ignored)"),
      expect.stringContaining(NO_PATH),
      expect.stringContaining(orphan("implementer")),
      expect.stringContaining(orphan("tester")),
    ]);
    expect(screen.getAllByTestId("lint-banner")).toHaveLength(1);

    // Editing: the live mirror takes over — still one row per defect × node.
    act(() => useEditStore.getState().addEdge(START_EDGE));
    expect(rows()).toEqual([expect.stringContaining("unknown field 'x' (ignored)")]);
  });

  it("lists every defect, with no cap (Q2)", () => {
    const p = pipeline();
    p.edges = [];
    for (let i = 0; i < 6; i++) {
      p.nodes.push({ id: `n${i}`, name: `n${i}`, type: "agent", interactive: false, inputs: [], outputs: [] });
    }
    seed({ pipeline: p, dirty: true });
    renderCanvas();
    // no path + (2 + 6) dead ends + (2 + 6) orphans
    expect(rows()).toHaveLength(17);
  });

  it("keeps the Show of the selected node lit after the click", () => {
    seed();
    renderCanvas();
    act(() => useEditStore.getState().deleteEdge(0));

    const show = screen.getByTestId("lint-banner-show-impl");
    expect(show.className).toContain("text-fg-4");
    fireEvent.click(show);

    expect(useEditStore.getState().selection).toEqual({ kind: "node", id: "impl" });
    const lit = screen.getByTestId("lint-banner-show-impl");
    expect(lit).toHaveAttribute("aria-current", "true");
    expect(lit.className).toContain("text-acc");
    expect(lit.className).toContain("underline");
    expect(screen.getByTestId("lint-banner-show-tester")).not.toHaveAttribute("aria-current");
  });

  it("while the wiring is taught: amber marks, no coherence row in the banner (Q4)", () => {
    seed({ diagnostics: ["unknown field 'x' (ignored)"] });
    renderCanvas({ coherenceTeaching: true });
    act(() => useEditStore.getState().deleteEdge(0));

    expect(rows()).toEqual([expect.stringContaining("unknown field 'x' (ignored)")]);
    expect(marksOf("impl")).toEqual(["orphan"]);
    expect(toneOf("impl")).toBe("await");
  });

  it("puts no mark on a run tab", () => {
    const p = pipeline();
    p.edges = p.edges.slice(1);
    seed({ id: "__run__r1", scope: "run", runId: "r1", pipeline: p, dirty: true });
    useEditStore.setState({ activeTabId: "__run__r1" });
    renderCanvas();
    expect(marksOf("impl")).toBeUndefined();
  });
});

// The mark itself (design Q1 = B): corner badge + one caption per defect.
function Wrapper({ children }: { children: React.ReactNode }) {
  return (
    <TooltipProvider>
      <ReactFlowProvider>{children}</ReactFlowProvider>
    </TooltipProvider>
  );
}

function nodeProps(data: Record<string, unknown>, status: NodeStatus = "pending") {
  return {
    id: "impl",
    data: {
      label: "implementer",
      nodeId: "impl",
      nodeType: "agent" as NodeType,
      status,
      inputs: [],
      outputs: [{ name: "out", side: "right" as const }],
      interactive: false,
      ...data,
    },
    type: "edit",
    selected: false,
    isConnectable: true,
    zIndex: 0,
    positionAbsoluteX: 0,
    positionAbsoluteY: 0,
    dragging: false,
    deletable: true,
    selectable: true,
    width: 160,
    height: 80,
  } as unknown as Parameters<typeof EditNode>[0];
}

describe("EditNode path-coherence mark", () => {
  it("shows a red TriangleAlert badge named by its defects, and one caption per defect", () => {
    render(<EditNode {...nodeProps({ pathMarks: ["dead_end", "orphan"] })} />, { wrapper: Wrapper });

    const badge = screen.getByRole("img", {
      name: "Dead end — never leads to End\nOrphan — not reachable from Start",
    });
    expect(badge.className).toContain("text-st-failed");
    expect(screen.getByTestId("node-corner-badge")).toContainElement(badge);
    expect(screen.getByTestId("node-path-caption-dead_end")).toHaveTextContent("Dead end — never leads to End");
    expect(screen.getByTestId("node-path-caption-orphan")).toHaveTextContent("Orphan — not reachable from Start");
    // No border or ring: the card keeps its neutral cadre.
    expect(screen.getByTestId("node-card").className).not.toContain("st-failed");
  });

  it("turns amber in teaching mode", () => {
    render(<EditNode {...nodeProps({ pathMarks: ["no_path"], pathMarkTone: "await" })} />, { wrapper: Wrapper });
    expect(screen.getByRole("img", { name: "No path from Start to End" }).className).toContain("text-st-await");
    expect(screen.getByTestId("node-path-caption-no_path").className).toContain("text-st-await");
  });

  it("renders nothing without a defect", () => {
    render(<EditNode {...nodeProps({})} />, { wrapper: Wrapper });
    expect(screen.queryByTestId("node-path-mark")).toBeNull();
    expect(screen.queryByTestId("node-path-captions")).toBeNull();
    expect(screen.queryByTestId("node-corner-badge")).toBeNull();
  });
});
