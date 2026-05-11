import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import PipelineInspector from "./PipelineInspector";
import type { LibraryPipelineEntry } from "../api";
import { useEditStore } from "../stores/editStore";
import { TooltipProvider } from "./ui/tooltip";

vi.mock("../api", () => ({
  fetchLibrary: vi.fn().mockResolvedValue([]),
  fetchLibraryPipelines: vi.fn().mockResolvedValue([]),
  saveLibraryPipeline: vi.fn().mockResolvedValue({ id: "my-pipeline", scope: "repo" }),
  deleteLibraryPipeline: vi.fn().mockResolvedValue(undefined),
  saveToLibrary: vi.fn().mockResolvedValue({}),
  deleteFromLibrary: vi.fn().mockResolvedValue(undefined),
}));

function seedTab(libraryBinding?: { id: string | null; scope: "repo" | "user" | null }) {
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
        diagnostics: [],
        dirty: false,
        externalDirty: false,
        libraryId: libraryBinding?.id ?? null,
        libraryScope: libraryBinding?.scope ?? null,
      },
    ],
    activeTabId: "p1",
    selection: { kind: "none", id: null },
  });
}

function renderInspector(libraryPipelines: LibraryPipelineEntry[] = []) {
  return render(
    <TooltipProvider>
      <PipelineInspector
        libraryPipelines={libraryPipelines}
        onLibraryChanged={() => {}}
      />
    </TooltipProvider>,
  );
}

beforeEach(() => {
  vi.clearAllMocks();
});

describe("PipelineInspector", () => {
  // Inline star buttons were removed from PipelineInspector — the canvas-level
  // PipelineStar is now the single source of truth (see PipelineStar.tsx).
  it("renders identity and does not show any inline star", () => {
    seedTab();
    renderInspector([]);

    expect(screen.getByText("Pipeline Inspector")).toBeInTheDocument();
    expect(screen.queryByTitle("Star as template")).not.toBeInTheDocument();
    expect(screen.queryByTitle("Remove from library")).not.toBeInTheDocument();
  });

  it("does not show an inline star even when the pipeline is in the library", () => {
    seedTab({ id: "my-pipeline", scope: "repo" });
    const starred: LibraryPipelineEntry[] = [
      { id: "my-pipeline", name: "My Pipeline", scope: "repo", node_count: 2, modified: null, yaml: "" },
    ];
    renderInspector(starred);

    expect(screen.queryByTitle("Star as template")).not.toBeInTheDocument();
    expect(screen.queryByTitle("Remove from library")).not.toBeInTheDocument();
  });

  it("shows a scope toggle only when the pipeline is in the library", () => {
    seedTab();
    const { rerender } = renderInspector([]);
    expect(screen.queryByTestId("pipeline-inspector-scope")).not.toBeInTheDocument();

    seedTab({ id: "my-pipeline", scope: "repo" });
    rerender(
      <TooltipProvider>
        <PipelineInspector
          libraryPipelines={[
            { id: "my-pipeline", name: "My Pipeline", scope: "repo", node_count: 2, modified: null, yaml: "" },
          ]}
          onLibraryChanged={() => {}}
        />
      </TooltipProvider>,
    );
    expect(screen.getByTestId("pipeline-inspector-scope")).toBeInTheDocument();
    // The currently-selected scope renders with the acc-coloured outline; the
    // other option is still visible so the user can flip.
    expect(screen.getByTestId("pipeline-inspector-scope-repo")).toBeInTheDocument();
    expect(screen.getByTestId("pipeline-inspector-scope-user")).toBeInTheDocument();
  });
});
