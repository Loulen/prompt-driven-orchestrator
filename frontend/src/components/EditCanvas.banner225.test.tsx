import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import EditCanvas from "./EditCanvas";
import { useEditStore, type OpenPipeline } from "../stores/editStore";
import { TooltipProvider } from "./ui/tooltip";

// jsdom has no ResizeObserver; ReactFlow's container measurement needs it
// (mirrors LintBannerDuplication.test.tsx).
globalThis.ResizeObserver = class {
  observe() {}
  unobserve() {}
  disconnect() {}
};

// Stub only the heavy <ReactFlow> canvas to a passthrough <div>. The diagnostics
// overlay banner and the star container are rendered OUTSIDE <ReactFlow> in
// EditCanvas, so collapsing the canvas body does not hide what's under test.
vi.mock("@xyflow/react", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@xyflow/react")>();
  return {
    ...actual,
    ReactFlow: ({ children }: { children?: React.ReactNode }) => (
      <div data-testid="reactflow-stub">{children}</div>
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
      id: "end",
      name: "End",
      type: "end" as const,
      interactive: false,
      inputs: [{ name: "result", repeated: false, side: "left" as const }],
      outputs: [],
    },
  ],
  edges: [
    {
      source: { node: "start", port: "user_prompt" },
      target: { node: "end", port: "result" },
    },
  ],
};

const DIAGNOSTIC = "unknown field 'auto_merge_resolver' (ignored)";

// An edit tab: `runId` unset, `scope` is a normal pipeline scope. This is the
// state in which the lint banner is a legitimate edit-mode affordance.
function editTab(): OpenPipeline {
  return {
    id: "p1",
    scope: "repo",
    pipeline: PIPELINE,
    prompts: {},
    diagnostics: [DIAGNOSTIC],
    dirty: false,
    externalDirty: false,
    libraryId: null,
    libraryScope: null,
  };
}

// A run tab: shaped exactly as `openRunPipeline` produces it — tabId prefixed
// `__run__`, `scope: "run"`, `runId` set. Carries the SAME diagnostics as the
// edit tab (run snapshots inherit them via fetchRunPipeline), which is precisely
// why the banner leaked into the run view before #225.
function runTab(): OpenPipeline {
  return {
    id: "__run__r1",
    scope: "run",
    pipeline: PIPELINE,
    prompts: {},
    diagnostics: [DIAGNOSTIC],
    dirty: false,
    externalDirty: false,
    runId: "r1",
    libraryId: null,
    libraryScope: null,
  };
}

function seedTab(tab: OpenPipeline) {
  useEditStore.setState({
    openTabs: [tab],
    activeTabId: tab.id,
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

beforeEach(() => {
  vi.clearAllMocks();
});

afterEach(() => {
  useEditStore.setState({ selection: { kind: "none", id: null } });
});

describe("#225 Part 1 — lint banner is gated to non-run tabs", () => {
  it("shows the lint banner on an edit tab (runId unset) with diagnostics", () => {
    seedTab(editTab());
    renderCanvas();

    const banners = screen.getAllByTestId("lint-banner");
    expect(banners).toHaveLength(1);
    expect(banners[0].textContent).toContain(DIAGNOSTIC);
  });

  it("suppresses the lint banner on a run tab even though the snapshot carries the same diagnostics", () => {
    seedTab(runTab());
    renderCanvas();

    expect(screen.queryByTestId("lint-banner")).not.toBeInTheDocument();
  });
});

describe("#225 Part 2 — star container outranks the lint banner (z-index regression guard)", () => {
  // jsdom does no layout/hit-testing, so the click-swallow itself cannot be
  // reproduced here (the real proof is the Layer-5 scenario
  // banner-run-gate-star-popover.md). This guards the one thing a unit test
  // CAN pin: the container that traps the popover's stacking context must sit
  // at z-20, above the z-10 lint-banner overlay.
  it("renders the star container at z-20 (not z-10) on an edit tab", () => {
    seedTab(editTab());
    renderCanvas();

    const container = screen.getByTestId("canvas-pipeline-star-container");
    expect(container.className).toContain("z-20");
    expect(container.className).not.toContain("z-10");
  });
});
