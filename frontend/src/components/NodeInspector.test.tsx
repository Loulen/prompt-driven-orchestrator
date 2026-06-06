import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import NodeInspector from "./NodeInspector";
import type { LibraryEntry } from "../api";
import { saveToLibrary, deleteFromLibrary } from "../api";
import { useEditStore } from "../stores/editStore";
import { TooltipProvider } from "./ui/tooltip";

function renderInspector(props: Parameters<typeof NodeInspector>[0]) {
  return render(
    <TooltipProvider>
      <NodeInspector {...props} />
    </TooltipProvider>,
  );
}

vi.mock("../api", () => ({
  fetchLibrary: vi.fn().mockResolvedValue([]),
  saveToLibrary: vi.fn().mockResolvedValue({}),
  deleteFromLibrary: vi.fn().mockResolvedValue(undefined),
  instantiateFromLibrary: vi.fn().mockResolvedValue({
    spec: {
      name: "reviewer",
      type: "doc-only",
      inputs: [],
      outputs: [],
      interactive: false,
    },
    prompt: "stub",
  }),
}));

const mockSave = vi.mocked(saveToLibrary);
const mockDelete = vi.mocked(deleteFromLibrary);

function seedTabWithReviewer(dirty: boolean, prompt = "Review this code.") {
  useEditStore.setState({
    openTabs: [
      {
        id: "p1",
        scope: "repo",
        pipeline: {
          name: "p1",
          version: "1.0",
          variables: {},
          nodes: [
            {
              id: "rv1",
              name: "reviewer",
              type: "doc-only",
              interactive: false,
              inputs: [{ name: "in", repeated: false, side: "left" }],
              outputs: [{ name: "out", repeated: false, side: "right" }],
              view: { x: 0, y: 0 },
            },
          ],
          edges: [],
        },
        prompts: { rv1: prompt },
        diagnostics: [],
        dirty,
        externalDirty: false,
      },
    ],
    activeTabId: "p1",
    selection: { kind: "node", id: "rv1" },
  });
}

function seedPooledReviewPipeline() {
  useEditStore.setState({
    openTabs: [
      {
        id: "p1",
        scope: "repo",
        pipeline: {
          name: "p1",
          version: "1.0",
          variables: {},
          nodes: [
            {
              id: "sec",
              name: "security-reviewer",
              type: "doc-only",
              interactive: false,
              inputs: [],
              outputs: [{ name: "review", repeated: false, side: "right" }],
              view: { x: 0, y: 0 },
            },
            {
              id: "perf",
              name: "perf-reviewer",
              type: "doc-only",
              interactive: false,
              inputs: [],
              outputs: [{ name: "review", repeated: false, side: "right" }],
              view: { x: 0, y: 100 },
            },
            {
              id: "impl",
              name: "implementer",
              type: "code-mutating",
              interactive: false,
              inputs: [],
              outputs: [
                {
                  name: "diff",
                  repeated: false,
                  side: "right",
                  frontmatter: { verdict: { type: "enum", allowed: ["PASS", "FAIL"] } },
                },
              ],
              view: { x: 200, y: 50 },
            },
          ],
          edges: [
            { source: { node: "sec", port: "review" }, target: { node: "impl", port: "review" } },
            { source: { node: "perf", port: "review" }, target: { node: "impl", port: "review" } },
          ],
        },
        prompts: { impl: "Implement." },
        diagnostics: [],
        dirty: false,
        externalDirty: false,
      },
    ],
    activeTabId: "p1",
    selection: { kind: "node", id: "impl" },
  });
}

beforeEach(() => {
  vi.clearAllMocks();
});

describe("NodeInspector — pooled emergent inputs (#153)", () => {
  it("shows one pooled input listing both contributing source nodes", () => {
    seedPooledReviewPipeline();
    renderInspector({ libraryEntries: [], onLibraryChanged: () => {} });

    const pooled = screen.getByTestId("pooled-input-review");
    expect(pooled).toHaveTextContent("review");
    expect(pooled).toHaveTextContent("security-reviewer");
    expect(pooled).toHaveTextContent("perf-reviewer");
  });

  it("shows the node ID", () => {
    seedPooledReviewPipeline();
    renderInspector({ libraryEntries: [], onLibraryChanged: () => {} });
    expect(screen.getByText("impl")).toBeInTheDocument();
  });

  it("shows the declared output port schema fields", () => {
    seedPooledReviewPipeline();
    renderInspector({ libraryEntries: [], onLibraryChanged: () => {} });
    // The output card for `diff` renders its frontmatter schema editor.
    expect(screen.getByTestId("output-port-card-diff")).toBeInTheDocument();
    expect(screen.getByDisplayValue("verdict")).toBeInTheDocument();
  });
});

describe("NodeInspector StarButton — library save is independent of pipeline save", () => {
  it("Save to library works when pipeline is dirty (no longer requires save first)", () => {
    seedTabWithReviewer(true, "Review this code.");
    renderInspector({ libraryEntries: [], onLibraryChanged: () => {} });

    const star = screen.getByTitle("Save to library");
    expect((star as HTMLButtonElement).disabled).toBe(false);

    fireEvent.click(star);
    expect(mockSave).toHaveBeenCalledTimes(1);
    expect(mockSave).toHaveBeenCalledWith({
      name: "reviewer",
      type: "doc-only",
      inputs: [{ name: "in", repeated: false, side: "left" }],
      outputs: [{ name: "out", repeated: false, side: "right" }],
      interactive: false,
      prompt: "Review this code.",
    });
  });

  it("Save to library sends node spec + prompt inline (no pipeline_id)", () => {
    seedTabWithReviewer(false, "v2 prompt");
    renderInspector({ libraryEntries: [], onLibraryChanged: () => {} });

    fireEvent.click(screen.getByTitle("Save to library"));

    const arg = mockSave.mock.calls[0][0];
    expect(arg.prompt).toBe("v2 prompt");
    expect(arg.name).toBe("reviewer");
    // Confirm the old call shape (positional nodeId, pipelineId) is gone.
    expect(mockSave.mock.calls[0]).toHaveLength(1);
  });

  it("opens the popover when node is already synced with library", () => {
    const synced: LibraryEntry = {
      name: "reviewer",
      type: "doc-only",
      inputs: [{ name: "in", repeated: false, side: "left" }],
      outputs: [{ name: "out", repeated: false, side: "right" }],
      interactive: false,
      prompt: "Review this code.",
    };
    seedTabWithReviewer(true, "Review this code.");
    renderInspector({ libraryEntries: [synced], onLibraryChanged: () => {} });

    const star = screen.getByTitle("In your library — synced");
    fireEvent.click(star);

    // Popover items appear; save was not invoked directly.
    expect(screen.getByText(/Remove from library/i)).toBeInTheDocument();
    expect(mockSave).not.toHaveBeenCalled();
    expect(mockDelete).not.toHaveBeenCalled();
  });
});
