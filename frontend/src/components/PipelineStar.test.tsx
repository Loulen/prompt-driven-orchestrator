import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import PipelineStar from "./PipelineStar";
import { saveLibraryPipeline, deleteLibraryPipeline } from "../api";
import type { LibraryPipelineEntry } from "../api";
import { useEditStore } from "../stores/editStore";
import type { PipelineDef } from "../types";
import { TooltipProvider } from "./ui/tooltip";

vi.mock("../api", () => ({
  fetchLibrary: vi.fn().mockResolvedValue([]),
  fetchLibraryPipelines: vi.fn().mockResolvedValue([]),
  saveLibraryPipeline: vi.fn().mockResolvedValue({ id: "my-pipeline" }),
  deleteLibraryPipeline: vi.fn().mockResolvedValue(undefined),
  saveRunPipeline: vi.fn().mockResolvedValue(undefined),
  savePipeline: vi.fn().mockResolvedValue(undefined),
  fetchPipeline: vi.fn(),
  fetchRunPipeline: vi.fn(),
  fetchPipelines: vi.fn().mockResolvedValue([]),
  deletePipeline: vi.fn().mockResolvedValue(undefined),
}));

const mockSaveLib = vi.mocked(saveLibraryPipeline);
const mockDeleteLib = vi.mocked(deleteLibraryPipeline);

const PIPELINE: PipelineDef = {
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
};

function libEntry(yaml = ""): LibraryPipelineEntry {
  return { id: "my-pipeline", name: "My Pipeline", node_count: 2, modified: null, yaml };
}

function renderStar(props: {
  syncState: "outline" | "synced" | "diverged";
  libraryEntry: LibraryPipelineEntry | null;
}) {
  return render(
    <TooltipProvider>
      <PipelineStar
        tabId="p1"
        pipeline={PIPELINE}
        syncState={props.syncState}
        libraryEntry={props.libraryEntry}
        onLibraryChanged={() => {}}
      />
    </TooltipProvider>,
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  useEditStore.setState({ openTabs: [], activeTabId: null });
});

describe("PipelineStar", () => {
  it("outline state: click saves to library directly without popover", () => {
    renderStar({ syncState: "outline", libraryEntry: null });

    expect(screen.queryByTestId("pipeline-star-popover")).not.toBeInTheDocument();
    expect(screen.queryByTestId("pipeline-star-diverged-dot")).not.toBeInTheDocument();

    fireEvent.click(screen.getByTestId("pipeline-star"));

    expect(mockSaveLib).toHaveBeenCalledTimes(1);
    const [name, yaml] = mockSaveLib.mock.calls[0];
    expect(name).toBe("My Pipeline");
    expect(yaml).toContain("name: My Pipeline");
  });

  it("synced state: click opens popover with Remove option only", () => {
    renderStar({ syncState: "synced", libraryEntry: libEntry() });

    fireEvent.click(screen.getByTestId("pipeline-star"));

    const popover = screen.getByTestId("pipeline-star-popover");
    expect(popover).toBeInTheDocument();
    expect(popover.textContent).toContain("Remove from library");
    expect(popover.textContent).not.toContain("Update library entry");
    expect(popover.textContent).not.toContain("Reset from library");
  });

  it("diverged state: shows divergence dot and offers all three options", () => {
    renderStar({ syncState: "diverged", libraryEntry: libEntry() });

    expect(screen.getByTestId("pipeline-star-diverged-dot")).toBeInTheDocument();

    fireEvent.click(screen.getByTestId("pipeline-star"));
    const popover = screen.getByTestId("pipeline-star-popover");
    expect(popover.textContent).toContain("Update library entry");
    expect(popover.textContent).toContain("Reset from library");
    expect(popover.textContent).toContain("Remove from library");
  });

  it("Remove from library calls deleteLibraryPipeline with the entry id", () => {
    renderStar({ syncState: "synced", libraryEntry: libEntry() });
    fireEvent.click(screen.getByTestId("pipeline-star"));
    fireEvent.click(screen.getByText("Remove from library"));

    expect(mockDeleteLib).toHaveBeenCalledTimes(1);
    expect(mockDeleteLib).toHaveBeenCalledWith("my-pipeline");
  });

  it("Update library entry pushes the current canvas YAML to the library", () => {
    renderStar({ syncState: "diverged", libraryEntry: libEntry("stale yaml") });
    fireEvent.click(screen.getByTestId("pipeline-star"));
    fireEvent.click(screen.getByText("Update library entry"));

    expect(mockSaveLib).toHaveBeenCalledTimes(1);
    const [name, yaml] = mockSaveLib.mock.calls[0];
    expect(name).toBe("My Pipeline");
    expect(yaml).toContain("name: My Pipeline");
    expect(yaml).not.toBe("stale yaml");
  });
});
