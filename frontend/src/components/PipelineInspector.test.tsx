import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import PipelineInspector from "./PipelineInspector";
import { saveLibraryPipeline, deleteLibraryPipeline } from "../api";
import type { LibraryPipelineEntry } from "../api";
import { useEditStore } from "../stores/editStore";
import { TooltipProvider } from "./ui/tooltip";

vi.mock("../api", () => ({
  fetchLibrary: vi.fn().mockResolvedValue([]),
  fetchLibraryPipelines: vi.fn().mockResolvedValue([]),
  saveLibraryPipeline: vi.fn().mockResolvedValue({ id: "my-pipeline" }),
  deleteLibraryPipeline: vi.fn().mockResolvedValue(undefined),
  saveToLibrary: vi.fn().mockResolvedValue({}),
  deleteFromLibrary: vi.fn().mockResolvedValue(undefined),
}));

const mockSaveLib = vi.mocked(saveLibraryPipeline);
const mockDeleteLib = vi.mocked(deleteLibraryPipeline);

function seedTab() {
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

describe("PipelineInspector star button", () => {
  it("shows unfilled star when pipeline is not in library", () => {
    seedTab();
    renderInspector([]);

    const star = screen.getByTitle("Star as template");
    expect(star).toBeInTheDocument();
  });

  it("calls saveLibraryPipeline when star is clicked on unstarred pipeline", () => {
    seedTab();
    renderInspector([]);

    fireEvent.click(screen.getByTitle("Star as template"));

    expect(mockSaveLib).toHaveBeenCalledTimes(1);
    const [name, yaml] = mockSaveLib.mock.calls[0];
    expect(name).toBe("My Pipeline");
    expect(yaml).toContain("name: My Pipeline");
  });

  it("shows filled star when pipeline is in library", () => {
    seedTab();
    const starred: LibraryPipelineEntry[] = [
      { id: "my-pipeline", name: "My Pipeline", node_count: 2, modified: null },
    ];
    renderInspector(starred);

    const star = screen.getByTitle("Remove from library");
    expect(star).toBeInTheDocument();
  });

  it("calls deleteLibraryPipeline when star is clicked on starred pipeline", () => {
    seedTab();
    const starred: LibraryPipelineEntry[] = [
      { id: "my-pipeline", name: "My Pipeline", node_count: 2, modified: null },
    ];
    renderInspector(starred);

    fireEvent.click(screen.getByTitle("Remove from library"));

    expect(mockDeleteLib).toHaveBeenCalledTimes(1);
    expect(mockDeleteLib).toHaveBeenCalledWith("my-pipeline");
  });
});
