import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import EditCanvas from "./EditCanvas";
import PipelineInfoPanel from "./PipelineInfoPanel";
import { useEditStore } from "../stores/editStore";
import { TooltipProvider } from "./ui/tooltip";

// jsdom has no ResizeObserver; ReactFlow's container measurement needs it
// (mirrors NodeDetailPanel.test.tsx).
globalThis.ResizeObserver = class {
  observe() {}
  unobserve() {}
  disconnect() {}
};

// Stub only the heavy <ReactFlow> canvas to a passthrough <div>, keeping the
// provider/hooks/Handle real (mirrors OrthogonalEdge.test.tsx's partial mock).
// The diagnostics overlay banner is rendered OUTSIDE <ReactFlow> in EditCanvas,
// so collapsing the canvas body does not hide the banner under test — it only
// removes jsdom fitView / zero-getBoundingClientRect flakiness.
vi.mock("@xyflow/react", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@xyflow/react")>();
  return {
    ...actual,
    ReactFlow: ({ children }: { children?: React.ReactNode }) => <div data-testid="reactflow-stub">{children}</div>,
  };
});

// EditCanvas (via usePipelineLibraryState) and PipelineInfoPanel both import the
// API client; stub the network so nothing fetches on mount.
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
}));

// Seed a tab with NO node selected and a non-empty diagnostics array — the exact
// state in which the duplicate banner appeared (#63). Pre-fix this co-mount
// produced two `lint-banner` nodes (canvas overlay + inspector copy); post-fix
// the canvas overlay is the single home. Since #948 the pipeline's settings live
// in Pipeline info, so that is the panel co-mounted beside the canvas.
function seedNoSelectionWithDiagnostics() {
  useEditStore.setState({
    openTabs: [
      {
        id: "p1",
        scope: "repo",
        pipeline: {
          name: "My Pipeline",
          version: "1.0",
          variables: {},
          nodes: [
            {
              id: "start",
              name: "Start",
              type: "start",
              interactive: false,
              inputs: [],
              outputs: [{ name: "user_prompt" }],
            },
            {
              id: "end",
              name: "End",
              type: "end",
              interactive: false,
              inputs: [{ name: "result" }],
              outputs: [],
            },
          ],
          edges: [
            {
              source: { node: "start", port: "user_prompt" },
              target: { node: "end", port: "result" },
            },
          ],
        },
        prompts: {},
        diagnostics: ["node 'reviewer': unknown node type 'merge', defaulting to 'agent'"],
        dirty: false,
        externalDirty: false,
        libraryId: null,
        libraryScope: null,
      },
    ],
    activeTabId: "p1",
    selection: { kind: "none", id: null },
  });
}

beforeEach(() => {
  vi.clearAllMocks();
  seedNoSelectionWithDiagnostics();
});

afterEach(() => {
  useEditStore.setState({ selection: { kind: "none", id: null } });
});

describe("LintBanner duplication (#63)", () => {
  it("renders exactly one lint banner when EditCanvas and Pipeline info are co-mounted with no node selected", () => {
    render(
      <TooltipProvider>
        <EditCanvas
          libraryEntries={[]}
          libraryPipelines={[]}
          onLibraryDelete={() => {}}
          onLibraryPipelinesChanged={() => {}}
        />
        <PipelineInfoPanel
          run={null}
          pipeline={useEditStore.getState().openTabs[0].pipeline}
          onClose={() => {}}
        />
      </TooltipProvider>,
    );

    // Both panels mounted: the canvas body (stub) and Pipeline info's settings.
    expect(screen.getByTestId("reactflow-stub")).toBeInTheDocument();
    expect(screen.getByTestId("pipeline-meta")).toBeInTheDocument();

    // The diagnostics surface exactly once — the floating canvas overlay.
    expect(screen.getAllByTestId("lint-banner")).toHaveLength(1);
  });
});

// ADR-0081 / gitlab #23: a daemon path-coherence diagnostic naming a node gets
// « Show », which selects that node; the « no path » row has no link.
describe("LintBanner Show on path-coherence diagnostics", () => {
  it("links the culprit node's row to the canvas selection", () => {
    const orphan = 'Not launchable: orphan node — node "worker" is not reachable from Start.';
    const noPath = "Not launchable: no path from Start to End.";
    useEditStore.setState((s) => ({
      openTabs: s.openTabs.map((t) => ({
        ...t,
        diagnostics: [noPath, orphan],
        pathDefects: [
          { kind: "no_path", message: noPath },
          { kind: "orphan", node_id: "worker", node_name: "worker", message: orphan },
        ],
      })),
    }));
    render(
      <TooltipProvider>
        <EditCanvas
          libraryEntries={[]}
          libraryPipelines={[]}
          onLibraryDelete={() => {}}
          onLibraryPipelinesChanged={() => {}}
        />
      </TooltipProvider>,
    );

    expect(screen.getByText(noPath)).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: "Show" })).toHaveLength(1);
    fireEvent.click(screen.getByTestId("lint-banner-show-worker"));
    expect(useEditStore.getState().selection).toEqual({ kind: "node", id: "worker" });
  });

  it("pairs each row with its own node even when two culprits share a name", () => {
    const dead = 'Not launchable: dead end — node "worker" never leads to End.';
    useEditStore.setState((s) => ({
      openTabs: s.openTabs.map((t) => ({
        ...t,
        diagnostics: ["an unrelated parse warning", dead, dead],
        pathDefects: [
          { kind: "dead_end", node_id: "w1", node_name: "worker", message: dead },
          { kind: "dead_end", node_id: "w2", node_name: "worker", message: dead },
        ],
      })),
    }));
    render(
      <TooltipProvider>
        <EditCanvas
          libraryEntries={[]}
          libraryPipelines={[]}
          onLibraryDelete={() => {}}
          onLibraryPipelinesChanged={() => {}}
        />
      </TooltipProvider>,
    );

    fireEvent.click(screen.getByTestId("lint-banner-show-w1"));
    expect(useEditStore.getState().selection).toEqual({ kind: "node", id: "w1" });
    fireEvent.click(screen.getByTestId("lint-banner-show-w2"));
    expect(useEditStore.getState().selection).toEqual({ kind: "node", id: "w2" });
  });
});
