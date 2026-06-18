import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import EditCanvas from "./EditCanvas";
import PipelineInspector from "./PipelineInspector";
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

// EditCanvas (via usePipelineLibraryState) and PipelineInspector both import the
// API client; stub the network so nothing fetches on mount.
vi.mock("../api", () => ({
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
// the canvas overlay is the single home.
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
              outputs: [{ name: "user_prompt", repeated: false, side: "right" }],
            },
            {
              id: "end",
              name: "End",
              type: "end",
              interactive: false,
              inputs: [{ name: "result", repeated: false, side: "left" }],
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
        diagnostics: ["node 'reviewer' receives edges from 2 code-mutating nodes without a Merge"],
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
  it("renders exactly one lint banner when EditCanvas and PipelineInspector are co-mounted with no node selected", () => {
    render(
      <TooltipProvider>
        <EditCanvas
          libraryEntries={[]}
          libraryPipelines={[]}
          onLibraryDelete={() => {}}
          onLibraryPipelinesChanged={() => {}}
        />
        <PipelineInspector libraryPipelines={[]} onLibraryChanged={() => {}} />
      </TooltipProvider>,
    );

    // Both panels mounted: the canvas body (stub) and the inspector header.
    expect(screen.getByTestId("reactflow-stub")).toBeInTheDocument();
    expect(screen.getByText("Pipeline Inspector")).toBeInTheDocument();

    // The diagnostics surface exactly once — the floating canvas overlay.
    expect(screen.getAllByTestId("lint-banner")).toHaveLength(1);
  });
});
