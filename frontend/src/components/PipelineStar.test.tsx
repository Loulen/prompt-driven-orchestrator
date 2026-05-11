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
  saveLibraryPipeline: vi.fn().mockResolvedValue({ id: "my-pipeline", scope: "repo" }),
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
  return { id: "my-pipeline", name: "My Pipeline", scope: "repo", node_count: 2, modified: null, yaml };
}

function renderStar(props: {
  syncState: "outline" | "synced" | "diverged";
  libraryEntry: LibraryPipelineEntry | null;
  pipeline?: PipelineDef;
}) {
  return render(
    <TooltipProvider>
      <PipelineStar
        tabId="p1"
        pipeline={props.pipeline ?? PIPELINE}
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
  it("outline state: click saves to library directly without popover, defaults to repo scope", () => {
    renderStar({ syncState: "outline", libraryEntry: null });

    expect(screen.queryByTestId("pipeline-star-popover")).not.toBeInTheDocument();
    expect(screen.queryByTestId("pipeline-star-diverged-dot")).not.toBeInTheDocument();

    fireEvent.click(screen.getByTestId("pipeline-star"));

    expect(mockSaveLib).toHaveBeenCalledTimes(1);
    const [name, yaml, , options] = mockSaveLib.mock.calls[0];
    expect(name).toBe("My Pipeline");
    expect(yaml).toContain("name: My Pipeline");
    // Default scope for fresh stars is repo — the user is in a concrete repo
    // so the most useful default is to commit the template there.
    expect(options?.scope).toBe("repo");
    // No libraryId yet — fresh star.
    expect(options?.id).toBeUndefined();
  });

  it("forwards the current tab's node prompts so library entries can spawn live runs", () => {
    useEditStore.setState({
      openTabs: [
        {
          id: "p1",
          scope: "repo",
          pipeline: PIPELINE,
          prompts: { start: "Kick things off.", end: "Wrap things up." },
          diagnostics: [],
          dirty: false,
          externalDirty: false,
          libraryId: null,
          libraryScope: null,
        },
      ],
      activeTabId: "p1",
    });

    renderStar({ syncState: "outline", libraryEntry: null });
    fireEvent.click(screen.getByTestId("pipeline-star"));

    expect(mockSaveLib).toHaveBeenCalledTimes(1);
    const [, , prompts] = mockSaveLib.mock.calls[0];
    expect(prompts).toEqual({
      start: "Kick things off.",
      end: "Wrap things up.",
    });
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
    const [name, yaml, , options] = mockSaveLib.mock.calls[0];
    expect(name).toBe("My Pipeline");
    expect(yaml).toContain("name: My Pipeline");
    expect(yaml).not.toBe("stale yaml");
    // Updating a known library entry must include its id, otherwise the
    // daemon would derive a new slug from `name` and orphan the old file.
    expect(options?.id).toBe("my-pipeline");
    expect(options?.scope).toBe("repo");
  });

  it("rename + update: still saves under the locked libraryId, not the new name's slug", () => {
    // Tab has already locked onto library id "my-pipeline"; the on-canvas name
    // has been renamed to "Renamed Pipeline". A diverged-state Update Library
    // entry must keep using the locked id so the file is overwritten in place
    // rather than producing a second `renamed-pipeline.yaml`.
    useEditStore.setState({
      openTabs: [
        {
          id: "p1",
          scope: "repo",
          pipeline: { ...PIPELINE, name: "Renamed Pipeline" },
          prompts: {},
          diagnostics: [],
          dirty: false,
          externalDirty: false,
          libraryId: "my-pipeline",
          libraryScope: "repo",
        },
      ],
      activeTabId: "p1",
    });

    renderStar({
      syncState: "diverged",
      libraryEntry: libEntry("stale yaml"),
      pipeline: { ...PIPELINE, name: "Renamed Pipeline" },
    });
    fireEvent.click(screen.getByTestId("pipeline-star"));
    fireEvent.click(screen.getByText("Update library entry"));

    expect(mockSaveLib).toHaveBeenCalledTimes(1);
    const [name, , , options] = mockSaveLib.mock.calls[0];
    expect(name).toBe("Renamed Pipeline");
    expect(options?.id).toBe("my-pipeline");
  });
});
