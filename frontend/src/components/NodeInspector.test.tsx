import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
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

function seedTabWithScript(prompt = "echo hi\n") {
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
              id: "sc1",
              name: "notify",
              type: "script",
              interactive: false,
              inputs: [],
              outputs: [{ name: "out", repeated: false, side: "right" }],
              view: { x: 0, y: 0 },
            },
          ],
          edges: [],
        },
        prompts: { sc1: prompt },
        diagnostics: [],
        dirty: false,
        externalDirty: false,
      },
    ],
    activeTabId: "p1",
    selection: { kind: "node", id: "sc1" },
  });
}

beforeEach(() => {
  vi.clearAllMocks();
});

describe("NodeInspector — script node surface (#248)", () => {
  it("shows the Script (bash) body editor and hides the model field", () => {
    seedTabWithScript();
    renderInspector({ libraryEntries: [], onLibraryChanged: () => {} });

    expect(screen.getByTestId("script-body")).toBeInTheDocument();
    expect(screen.getByTestId("script-help")).toBeInTheDocument();
    // A script launches no agent — the model field must be absent.
    expect(screen.queryByTestId("node-model-trigger")).toBeNull();
  });

  it("shows a static script type label, not the doc-only/code-mutating toggle", () => {
    seedTabWithScript();
    renderInspector({ libraryEntries: [], onLibraryChanged: () => {} });
    expect(screen.getByTestId("script-type-label")).toBeInTheDocument();
  });

  it("persists edits to the bash body and marks the tab dirty", () => {
    seedTabWithScript("echo old\n");
    renderInspector({ libraryEntries: [], onLibraryChanged: () => {} });

    const body = screen.getByTestId("script-body");
    fireEvent.change(body, { target: { value: "curl -X POST $PDO_DAEMON_URL\n" } });

    const tab = useEditStore.getState().openTabs[0];
    expect(tab.prompts["sc1"]).toBe("curl -X POST $PDO_DAEMON_URL\n");
    expect(tab.dirty).toBe(true);
  });
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

describe("NodeInspector — per-node model field (#296/#324)", () => {
  it("writes the picked model onto the node and marks the tab dirty", async () => {
    const user = userEvent.setup();
    seedTabWithReviewer(false, "Review this code.");
    renderInspector({ libraryEntries: [], onLibraryChanged: () => {} });

    await user.click(screen.getByTestId("node-model-trigger"));
    await user.click(await screen.findByTestId("node-model-option-opus"));

    const tab = useEditStore.getState().openTabs[0];
    expect(tab.pipeline.nodes[0].model).toBe("opus");
    expect(tab.dirty).toBe(true);
  });

  it("clears the model to null via Default (stays unset, never serialized)", async () => {
    const user = userEvent.setup();
    seedTabWithReviewer(false, "Review this code.");
    // Seed a model so we can watch it clear.
    useEditStore.getState().updateNode("rv1", { model: "opus" });
    renderInspector({ libraryEntries: [], onLibraryChanged: () => {} });

    expect(screen.getByTestId("node-model-trigger")).toHaveTextContent("opus");

    await user.click(screen.getByTestId("node-model-trigger"));
    await user.click(await screen.findByTestId("node-model-option-default"));
    expect(useEditStore.getState().openTabs[0].pipeline.nodes[0].model).toBeNull();
  });

  it("renders a seeded alias on the trigger", () => {
    seedTabWithReviewer(false, "Review this code.");
    useEditStore.getState().updateNode("rv1", { model: "haiku" });
    renderInspector({ libraryEntries: [], onLibraryChanged: () => {} });
    expect(screen.getByTestId("node-model-trigger")).toHaveTextContent("haiku");
  });

  it("renders a seeded arbitrary full id on the trigger (free text survives)", () => {
    seedTabWithReviewer(false, "Review this code.");
    useEditStore.getState().updateNode("rv1", { model: "claude-opus-4-8" });
    renderInspector({ libraryEntries: [], onLibraryChanged: () => {} });
    expect(screen.getByTestId("node-model-trigger")).toHaveTextContent("claude-opus-4-8");
  });
});

// #339: self-feeding node — the self-edge pools as an input source even though
// no edge is clickable on the canvas; its auto-materialized bounded region makes
// the × go through the destroy-loop confirmation.
function seedSelfFeedPipeline() {
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
              id: "cc1",
              name: "cycler",
              type: "doc-only",
              interactive: false,
              inputs: [],
              outputs: [{ name: "in", repeated: false, side: "right" }],
              view: { x: 0, y: 0 },
            },
          ],
          edges: [
            { source: { node: "cc1", port: "in" }, target: { node: "cc1", port: "in" } },
          ],
          loops: [{ id: "self_loop", kind: "bounded", members: ["cc1"], max_iter: 3 }],
        },
        prompts: { cc1: "Loop." },
        diagnostics: [],
        dirty: false,
        externalDirty: false,
      },
    ],
    activeTabId: "p1",
    selection: { kind: "node", id: "cc1" },
  });
}

describe("NodeInspector — per-source input delete (#339)", () => {
  it("deletes a non-cycle edge immediately and keeps the panel open on the node", () => {
    seedPooledReviewPipeline();
    renderInspector({ libraryEntries: [], onLibraryChanged: () => {} });

    fireEvent.click(screen.getByTestId("pooled-input-review-delete-sec"));

    const state = useEditStore.getState();
    // The sec → impl edge is gone; perf → impl survives.
    expect(state.openTabs[0].pipeline.edges).toEqual([
      { source: { node: "perf", port: "review" }, target: { node: "impl", port: "review" } },
    ]);
    // Selection kept → the inspector does not self-close.
    expect(state.selection).toEqual({ kind: "node", id: "impl" });
    expect(screen.getByTestId("pooled-input-review")).toBeInTheDocument();
    expect(screen.queryByTestId("destroy-loop-confirm")).toBeNull();
  });

  it("indices stay correct after a prior delete (re-derived each render)", () => {
    seedPooledReviewPipeline();
    renderInspector({ libraryEntries: [], onLibraryChanged: () => {} });

    fireEvent.click(screen.getByTestId("pooled-input-review-delete-sec"));
    // After the first delete the perf edge shifted to index 0 — the re-derived
    // × must delete IT, not a stale index.
    fireEvent.click(screen.getByTestId("pooled-input-review-delete-perf"));

    expect(useEditStore.getState().openTabs[0].pipeline.edges).toHaveLength(0);
    expect(useEditStore.getState().selection).toEqual({ kind: "node", id: "impl" });
  });

  it("self-edge (last cycle): × opens DestroyLoopModal; cancel leaves edge and loop", () => {
    seedSelfFeedPipeline();
    renderInspector({ libraryEntries: [], onLibraryChanged: () => {} });

    fireEvent.click(screen.getByTestId("pooled-input-in-delete-cc1"));

    expect(screen.getByTestId("destroy-loop-confirm")).toBeInTheDocument();
    fireEvent.click(screen.getByTestId("destroy-loop-cancel"));

    const tab = useEditStore.getState().openTabs[0];
    expect(tab.pipeline.edges).toHaveLength(1);
    expect(tab.pipeline.loops).toHaveLength(1);
    expect(screen.queryByTestId("destroy-loop-confirm")).toBeNull();
  });

  it("self-edge (last cycle): confirm deletes the edge AND the loops: entry, panel stays open", () => {
    seedSelfFeedPipeline();
    renderInspector({ libraryEntries: [], onLibraryChanged: () => {} });

    fireEvent.click(screen.getByTestId("pooled-input-in-delete-cc1"));
    fireEvent.click(screen.getByTestId("destroy-loop-confirm"));

    const state = useEditStore.getState();
    expect(state.openTabs[0].pipeline.edges).toHaveLength(0);
    expect(state.openTabs[0].pipeline.loops ?? []).toHaveLength(0);
    expect(state.selection).toEqual({ kind: "node", id: "cc1" });
    expect(screen.queryByTestId("destroy-loop-confirm")).toBeNull();
  });

  it("readOnly hides every × (archived gate) while rows still render", () => {
    seedPooledReviewPipeline();
    renderInspector({ libraryEntries: [], onLibraryChanged: () => {}, readOnly: true });

    expect(screen.getByTestId("pooled-input-review")).toBeInTheDocument();
    expect(screen.queryByTestId("pooled-input-review-delete-sec")).toBeNull();
    expect(screen.queryByTestId("pooled-input-review-delete-perf")).toBeNull();
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
      // #345/#296: the library is model-aware; a model-less node sends null.
      model: null,
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
