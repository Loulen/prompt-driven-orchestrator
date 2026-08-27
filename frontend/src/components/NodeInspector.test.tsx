import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import NodeInspector from "./NodeInspector";
import type { LibraryEntry } from "../api";
import { saveToLibrary, deleteFromLibrary, fetchSettings } from "../api";
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
  // #586: the harness pin picker now fetches /settings for its dynamic option
  // list. Resolve it with the embedded floor so the picker offers claude/opencode.
  fetchSettings: vi.fn().mockResolvedValue({
    harness_descriptors: {
      path: null,
      names: ["claude", "opencode", "copilot"],
      // #616/ADR-0053: the served catalogue drives the model/effort pickers and the
      // effort greying. claude offers models + efforts (effort axis present);
      // opencode offers a model list but no effort axis (greyed); copilot offers
      // efforts but NO model catalogue yet (#629), so its model control is the
      // free-text field — the shape the #617 FP caught leaking a value.
      harnesses: [
        {
          name: "claude",
          source: "builtin",
          installed: true,
          models: ["sonnet", "opus", "haiku", "opusplan"],
          efforts: ["low", "medium", "high", "xhigh", "max"],
          has_effort: true,
          version: "claude 1.0",
        },
        {
          name: "opencode",
          source: "builtin",
          installed: true,
          models: ["openrouter/foo"],
          efforts: [],
          has_effort: false,
          version: "opencode 1.18",
        },
        {
          name: "copilot",
          source: "builtin",
          installed: true,
          models: [],
          efforts: ["none", "minimal", "low", "medium", "high", "xhigh", "max"],
          has_effort: true,
          version: "copilot 1.0.80",
        },
      ],
      rejected: [],
      reason: null,
    },
  }),
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
    // #424: and so must the effort field, for the same reason. Absent, not
    // disabled — the house masks controls rather than greying them out.
    expect(screen.queryByRole("radiogroup", { name: "Effort" })).toBeNull();
    expect(screen.queryByTestId("node-effort-option-low")).toBeNull();
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

describe("NodeInspector — per-node model field (#296/#324, #616)", () => {
  // #616: the model picker renders the SERVED catalogue, fetched async via
  // `useHarnessCatalog`. Queries `findBy*` so the fetch (claude's models) resolves
  // before the dropdown trigger exists.
  it("writes the picked model onto the node and marks the tab dirty", async () => {
    const user = userEvent.setup();
    seedTabWithReviewer(false, "Review this code.");
    renderInspector({ libraryEntries: [], onLibraryChanged: () => {} });

    await user.click(await screen.findByTestId("node-model-trigger"));
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

    expect(await screen.findByTestId("node-model-trigger")).toHaveTextContent("opus");

    await user.click(screen.getByTestId("node-model-trigger"));
    await user.click(await screen.findByTestId("node-model-option-default"));
    expect(useEditStore.getState().openTabs[0].pipeline.nodes[0].model).toBeNull();
  });

  it("renders a seeded served id on the trigger", async () => {
    seedTabWithReviewer(false, "Review this code.");
    useEditStore.getState().updateNode("rv1", { model: "haiku" });
    renderInspector({ libraryEntries: [], onLibraryChanged: () => {} });
    expect(await screen.findByTestId("node-model-trigger")).toHaveTextContent("haiku");
  });

  it("renders a seeded arbitrary full id on the trigger (free text survives)", async () => {
    seedTabWithReviewer(false, "Review this code.");
    useEditStore.getState().updateNode("rv1", { model: "claude-opus-4-8" });
    renderInspector({ libraryEntries: [], onLibraryChanged: () => {} });
    expect(await screen.findByTestId("node-model-trigger")).toHaveTextContent("claude-opus-4-8");
  });
});

describe("NodeInspector — per-node effort field (#424, #616)", () => {
  it("writes the picked effort onto the node and marks the tab dirty", async () => {
    const user = userEvent.setup();
    seedTabWithReviewer(false, "Review this code.");
    renderInspector({ libraryEntries: [], onLibraryChanged: () => {} });

    await user.click(await screen.findByTestId("node-effort-option-low"));

    const tab = useEditStore.getState().openTabs[0];
    expect(tab.pipeline.nodes[0].effort).toBe("low");
    expect(tab.dirty).toBe(true);
  });

  it("clears the effort to null via Default (stays unset, never serialized)", async () => {
    const user = userEvent.setup();
    seedTabWithReviewer(false, "Review this code.");
    useEditStore.getState().updateNode("rv1", { effort: "high" });
    renderInspector({ libraryEntries: [], onLibraryChanged: () => {} });

    expect(await screen.findByTestId("node-effort-option-high")).toHaveAttribute(
      "aria-checked",
      "true",
    );

    await user.click(screen.getByTestId("node-effort-option-default"));
    expect(useEditStore.getState().openTabs[0].pipeline.nodes[0].effort).toBeNull();
  });

  it("is orthogonal to the model — setting one leaves the other alone", async () => {
    const user = userEvent.setup();
    seedTabWithReviewer(false, "Review this code.");
    useEditStore.getState().updateNode("rv1", { model: "opus" });
    renderInspector({ libraryEntries: [], onLibraryChanged: () => {} });

    await user.click(await screen.findByTestId("node-effort-option-max"));

    const node = useEditStore.getState().openTabs[0].pipeline.nodes[0];
    expect(node.effort).toBe("max");
    expect(node.model).toBe("opus");
  });

  it("renders a seeded unknown level in the pass-through segment (free text survives)", () => {
    seedTabWithReviewer(false, "Review this code.");
    useEditStore.getState().updateNode("rv1", { effort: "turbo" });
    renderInspector({ libraryEntries: [], onLibraryChanged: () => {} });
    expect(screen.getByTestId("node-effort-option-passthrough")).toHaveTextContent("turbo");
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
      // #424: effort-aware too; an effort-less node sends null.
      effort: null,
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

  it("Save to library preserves nonblank output instructions", () => {
    seedTabWithReviewer(false);
    const node = useEditStore.getState().openTabs[0].pipeline.nodes[0];
    node.outputs = [
      { ...node.outputs[0], instructions: "Return a concise verdict." },
      { name: "empty", repeated: false, side: "right", instructions: "   " },
    ];
    renderInspector({ libraryEntries: [], onLibraryChanged: () => {} });

    fireEvent.click(screen.getByTitle("Save to library"));

    expect(mockSave.mock.calls[0][0].outputs).toEqual([
      {
        name: "out",
        repeated: false,
        side: "right",
        instructions: "Return a concise verdict.",
      },
      { name: "empty", repeated: false, side: "right" },
    ]);
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

// #550/ADR-0046: the harness axis in the node inspector.
function seedNode(node: Record<string, unknown>) {
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
              id: "n1",
              name: "worker",
              type: "doc-only",
              interactive: false,
              inputs: [{ name: "in", repeated: false, side: "left" }],
              outputs: [{ name: "out", repeated: false, side: "right" }],
              view: { x: 0, y: 0 },
              ...node,
            },
          ],
          edges: [],
        },
        prompts: { n1: "do the thing" },
        diagnostics: [],
        dirty: false,
        externalDirty: false,
      },
    ],
    activeTabId: "p1",
    selection: { kind: "node", id: "n1" },
  });
}

describe("NodeInspector — harness axis (#550, ADR-0046)", () => {
  beforeEach(() => {
    seedNode({});
  });

  it("resolves to the claude floor when the node has no pin", async () => {
    seedNode({});
    renderInspector({ libraryEntries: [], onLibraryChanged: () => {} });
    expect(screen.getByTestId("node-harness-resolved")).toHaveTextContent("claude");
    // #616: claude serves an effort axis → the picker is ENABLED and renders its
    // served stops (here, "high"). `findBy*` awaits the async catalogue fetch.
    expect(await screen.findByTestId("node-effort-option-high")).not.toBeDisabled();
  });

  it("shows the pinned harness as resolved and greys the effort picker on opencode", async () => {
    seedNode({ pin_harness: "opencode" });
    renderInspector({ libraryEntries: [], onLibraryChanged: () => {} });
    expect(screen.getByTestId("node-harness-resolved")).toHaveTextContent("opencode");
    // #616/AC #3: opencode's SERVED `has_effort` is false → the picker is greyed.
    // opencode enumerates no stops, so assert on the always-present Default segment
    // and the group — never on a served level (there is none). Assert `disabled`,
    // never `.value`.
    await waitFor(() => expect(vi.mocked(fetchSettings)).toHaveBeenCalled());
    expect(screen.getByTestId("node-effort-option-default")).toBeDisabled();
    expect(screen.getByRole("radiogroup", { name: "Effort" })).toHaveAttribute(
      "aria-disabled",
      "true",
    );
  });

  it("pinning a harness writes pin_harness onto the node", async () => {
    const user = userEvent.setup();
    seedNode({});
    renderInspector({ libraryEntries: [], onLibraryChanged: () => {} });
    // #586: the pin is a custom sectioned dropdown; opencode is offered even before
    // /settings answers (the embedded-floor fallback). Open it and pick opencode.
    await user.click(screen.getByTestId("node-harness-select"));
    await user.click(await screen.findByTestId("node-harness-select-option-opencode"));
    const node = useEditStore.getState().openTabs[0].pipeline.nodes[0];
    expect(node.pin_harness).toBe("opencode");
    // A green fetch settles the dynamic catalog; flush it inside act so the
    // trailing setState doesn't leak past the test.
    await waitFor(() =>
      expect(vi.mocked(fetchSettings)).toHaveBeenCalled(),
    );
  });

  it("hides the harness selector for a script node", () => {
    seedNode({ type: "script" });
    renderInspector({ libraryEntries: [], onLibraryChanged: () => {} });
    expect(screen.queryByTestId("node-harness")).toBeNull();
  });
});

// #617 FP finding 3: the inspector is ONE component reused as the selection moves,
// so a picker holding its own state can show — and then WRITE — the previously
// selected node's model. That is an `opencode` slug landing on a `copilot` node,
// which is exactly what "a model means nothing outside its harness" (#550/ADR-0046)
// forbids. The journey below is the one the FP walked.
describe("NodeInspector — the model field does not follow the selection (#617)", () => {
  function seedTwoNodes() {
    const base = {
      type: "doc-only" as const,
      interactive: false,
      inputs: [],
      outputs: [{ name: "out", repeated: false, side: "right" as const }],
      view: { x: 0, y: 0 },
    };
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
              // Pinned to a harness with no served model catalogue so BOTH nodes
              // render the free-text control — the shape that carried the value.
              {
                ...base,
                id: "opc",
                name: "opencode-ish",
                pin_harness: "copilot",
                model: "openrouter/anthropic/claude-haiku-4.5",
              },
              { ...base, id: "cop", name: "copilot", pin_harness: "copilot" },
            ],
            edges: [],
          },
          prompts: { opc: "a", cop: "b" },
          diagnostics: [],
          dirty: false,
          externalDirty: false,
        },
      ],
      activeTabId: "p1",
      selection: { kind: "node", id: "opc" },
    });
  }

  it("shows nothing on a node that carries no model, whatever was selected before", async () => {
    seedTwoNodes();
    const { rerender } = renderInspector({ libraryEntries: [], onLibraryChanged: () => {} });

    const field = async () => (await screen.findByTestId("node-model-input")) as HTMLInputElement;
    expect((await field()).value).toBe("openrouter/anthropic/claude-haiku-4.5");

    useEditStore.setState({ selection: { kind: "node", id: "cop" } });
    rerender(
      <TooltipProvider>
        <NodeInspector libraryEntries={[]} onLibraryChanged={() => {}} />
      </TooltipProvider>,
    );

    expect((await field()).value).toBe("");
  });

  it("a focus-and-blur on the second node writes no model onto it", async () => {
    const user = userEvent.setup();
    seedTwoNodes();
    renderInspector({ libraryEntries: [], onLibraryChanged: () => {} });
    await screen.findByTestId("node-model-input");

    useEditStore.setState({ selection: { kind: "node", id: "cop" } });
    await user.click(await screen.findByTestId("node-model-input"));
    await user.click(screen.getByTestId("node-harness-resolved")); // click away

    const cop = useEditStore.getState().openTabs[0].pipeline.nodes[1];
    expect(cop.model ?? null).toBeNull();
    // …and the tab is not dirtied by a field the user only looked at.
    expect(useEditStore.getState().openTabs[0].dirty).toBe(false);
  });
});
