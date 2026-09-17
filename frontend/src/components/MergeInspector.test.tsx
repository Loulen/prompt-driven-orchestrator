import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, expect, beforeEach, vi } from "vitest";
import MergeInspector from "./MergeInspector";
import { fetchSettings } from "../api";
import { useEditStore } from "../stores/editStore";
import type { PipelineDef, NodeDef } from "../types";

// #616: the merge inspector now offers the SAME harness picker as any agent node
// (correctif 9), so it fetches the served catalogue via `useHarnessCatalog`. Mock
// `/settings` with claude serving models + efforts so its (unpinned → claude floor)
// pickers render their dropdowns.
vi.mock("../api", () => ({
  fetchSettings: vi.fn().mockResolvedValue({
    harness_descriptors: {
      path: null,
      names: ["claude"],
      harnesses: [
        {
          name: "claude",
          source: "builtin",
          installed: true,
          // #798: two models of the SAME harness with disjoint effort support —
          // the reported regression pair. `union-alpha` supports ONLY `off`.
          models: ["sonnet", "opus", "haiku", "union-alpha", "GPT5.4"],
          efforts: ["low", "medium", "high", "xhigh", "max"],
          model_efforts: {
            "union-alpha": ["off"],
            "GPT5.4": ["off", "low", "medium", "high", "xhigh"],
          },
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
      ],
      rejected: [],
      reason: null,
    },
  }),
}));

function makeMergeNode(overrides?: Partial<NodeDef>): NodeDef {
  return {
    id: "mg1",
    name: "merge-point",
    type: "merge",
    inputs: [{ name: "branches", repeated: true, side: "left" }],
    outputs: [{ name: "merged", repeated: false, side: "right" }],
    interactive: false,
    ...overrides,
  };
}

function makePipeline(node: NodeDef): PipelineDef {
  return {
    name: "test-pipeline",
    variables: {},
    nodes: [node],
    edges: [],
  };
}

function setStoreState(node: NodeDef) {
  const pipeline = makePipeline(node);
  useEditStore.setState({
    openTabs: [
      {
        id: "tab1",
        scope: "repo",
        pipeline,
        prompts: {},
        diagnostics: [],
        dirty: false,
        externalDirty: false,
      },
    ],
    activeTabId: "tab1",
    selection: { kind: "node", id: node.id },
  });
}

describe("MergeInspector", () => {
  beforeEach(() => {
    useEditStore.setState({
      openTabs: [],
      activeTabId: null,
      selection: { kind: "none", id: null },
    });
  });

  it("renders nothing when no tab is active", () => {
    const { container } = render(<MergeInspector />);
    expect(container.firstChild).toBeNull();
  });

  it("renders the inspector header", () => {
    setStoreState(makeMergeNode());
    render(<MergeInspector />);
    expect(screen.getByText("Merge Inspector")).toBeInTheDocument();
  });

  it("displays the node ID", () => {
    setStoreState(makeMergeNode());
    render(<MergeInspector />);
    expect(screen.getByText("mg1")).toBeInTheDocument();
  });

  it("displays the node name in an editable field", () => {
    setStoreState(makeMergeNode());
    render(<MergeInspector />);
    const nameInput = screen.getByDisplayValue("merge-point");
    expect(nameInput).toBeInTheDocument();
  });

  it("displays port labels", () => {
    setStoreState(makeMergeNode());
    render(<MergeInspector />);
    expect(screen.getByText("branches (repeated)")).toBeInTheDocument();
    expect(screen.getByText("merged")).toBeInTheDocument();
  });

  it("updates the name when changed", () => {
    setStoreState(makeMergeNode());
    render(<MergeInspector />);
    const nameInput = screen.getByDisplayValue("merge-point");
    fireEvent.change(nameInput, { target: { value: "new-merge" } });
    const tab = useEditStore.getState().openTabs[0];
    const node = tab.pipeline.nodes.find((n) => n.id === "mg1");
    expect(node?.name).toBe("new-merge");
  });

  it("displays the merge behavior description", () => {
    setStoreState(makeMergeNode());
    render(<MergeInspector />);
    expect(screen.getByText(/Merge nodes wait for all upstream/)).toBeInTheDocument();
  });

  // #296/#324: a merge node spawns an agent, so its model is settable here too.
  it("writes the picked model onto the merge node", async () => {
    const user = userEvent.setup();
    setStoreState(makeMergeNode());
    render(<MergeInspector />);
    await user.click(await screen.findByTestId("merge-model-trigger"));
    await user.click(await screen.findByTestId("merge-model-option-opus"));
    const node = useEditStore.getState().openTabs[0].pipeline.nodes.find((n) => n.id === "mg1");
    expect(node?.model).toBe("opus");
  });

  it("renders a seeded model on the trigger", async () => {
    setStoreState(makeMergeNode({ model: "sonnet" }));
    render(<MergeInspector />);
    expect(await screen.findByTestId("merge-model-trigger")).toHaveTextContent("sonnet");
  });

  // #424: a merge node IS a NodeDef routed through `spawn_node`, so it carries an
  // effort — unguarded, unlike a script node. (The `__merge_resolver__` infra
  // session is a different thing with a confusingly similar name and never has
  // one.)
  it("exposes the Effort control unconditionally and writes the picked level", async () => {
    const user = userEvent.setup();
    setStoreState(makeMergeNode());
    render(<MergeInspector />);

    expect(screen.getByRole("radiogroup", { name: "Effort" })).toBeInTheDocument();
    await user.click(await screen.findByTestId("merge-effort-option-medium"));

    const node = useEditStore.getState().openTabs[0].pipeline.nodes.find((n) => n.id === "mg1");
    expect(node?.effort).toBe("medium");
  });

  it("renders a seeded effort as the checked segment", async () => {
    setStoreState(makeMergeNode({ effort: "xhigh" }));
    render(<MergeInspector />);
    expect(await screen.findByTestId("merge-effort-option-xhigh")).toHaveAttribute(
      "aria-checked",
      "true",
    );
  });

  // #616 (correctif 9): a merge node exposes the SAME harness picker as any agent
  // node, and its effort picker follows the SAME served greying rule.
  it("offers a harness picker and writes pin_harness", async () => {
    const user = userEvent.setup();
    setStoreState(makeMergeNode());
    render(<MergeInspector />);
    // Resolves to the claude floor with no pin.
    expect(screen.getByTestId("merge-harness-resolved")).toHaveTextContent("claude");
    await user.click(await screen.findByTestId("merge-harness-select"));
    await user.click(await screen.findByTestId("merge-harness-select-option-claude"));
    const node = useEditStore.getState().openTabs[0].pipeline.nodes.find((n) => n.id === "mg1");
    expect(node?.pin_harness).toBe("claude");
  });

  it("greys the effort picker on a harness with no effort axis (served fact)", async () => {
    // Pinned on opencode, whose SERVED `has_effort` is false → the picker is greyed,
    // the same rule NodeInspector applies (correctif 9). opencode enumerates no
    // stops, so assert on the always-present Default segment and the group.
    setStoreState(makeMergeNode({ pin_harness: "opencode" }));
    render(<MergeInspector />);
    expect(screen.getByTestId("merge-harness-resolved")).toHaveTextContent("opencode");
    await waitFor(() => expect(vi.mocked(fetchSettings)).toHaveBeenCalled());
    expect(screen.getByTestId("merge-effort-option-default")).toBeDisabled();
    expect(screen.getByRole("radiogroup", { name: "Effort" })).toHaveAttribute(
      "aria-disabled",
      "true",
    );
  });

  // #798: the effort offer follows the node's SELECTED model. The reported pair:
  // `union-alpha` supports ONLY `off`; `GPT5.4` the full off..xhigh scale.
  describe("per-model effort support (#798)", () => {
    function node() {
      return useEditStore.getState().openTabs[0].pipeline.nodes.find((n) => n.id === "mg1");
    }

    it("a supported (model, effort) pair renders checked with no warning", async () => {
      setStoreState(makeMergeNode({ model: "GPT5.4", effort: "low" }));
      render(<MergeInspector />);
      expect(await screen.findByTestId("merge-effort-option-low")).toHaveAttribute(
        "aria-checked",
        "true",
      );
      // GPT5.4's own list — xhigh is supported for THIS model, no warning.
      expect(screen.getByTestId("merge-effort-option-xhigh")).toBeInTheDocument();
      expect(screen.queryByTestId("merge-effort-unsupported")).toBeNull();
    });

    it("a stored effort unsupported by the NEW model warns, stays visible, and is not silently deleted", async () => {
      const user = userEvent.setup();
      setStoreState(makeMergeNode({ model: "GPT5.4", effort: "low" }));
      render(<MergeInspector />);
      await screen.findByTestId("merge-effort-option-low");

      // Switch the model to union-alpha (off-only): the stored `low` is now
      // unsupported. It must NOT persist as an ordinary supported choice, and
      // must NOT be silently deleted either (ADR-0001).
      await user.click(screen.getByTestId("merge-model-trigger"));
      await user.click(await screen.findByTestId("merge-model-option-union-alpha"));

      expect(node()?.model).toBe("union-alpha");
      expect(node()?.effort).toBe("low"); // preserved, never silently deleted
      // Not offered as a supported option any more…
      expect(screen.queryByTestId("merge-effort-option-low")).toBeNull();
      // …but kept visible, flagged unsupported, with the way out.
      const extra = screen.getByTestId("merge-effort-option-passthrough");
      expect(extra).toHaveTextContent("low");
      expect(extra).toHaveAttribute("aria-checked", "true");
      expect(extra).toHaveAttribute("data-unsupported", "true");
      expect(screen.getByTestId("merge-effort-unsupported")).toBeInTheDocument();

      // The user resolves it explicitly: Default resets.
      await user.click(screen.getByTestId("merge-effort-option-default"));
      expect(node()?.effort).toBeNull();
    });

    it("a supported effort of the new model can be picked explicitly", async () => {
      const user = userEvent.setup();
      setStoreState(makeMergeNode({ model: "GPT5.4", effort: "low" }));
      render(<MergeInspector />);
      await screen.findByTestId("merge-effort-option-low");
      await user.click(screen.getByTestId("merge-model-trigger"));
      await user.click(await screen.findByTestId("merge-model-option-union-alpha"));
      await user.click(await screen.findByTestId("merge-effort-option-off"));
      expect(node()?.effort).toBe("off");
      expect(screen.queryByTestId("merge-effort-unsupported")).toBeNull();
    });

    it("a model without a key retains the harness's global efforts (fallback, no exceptions)", async () => {
      setStoreState(makeMergeNode({ model: "sonnet", effort: "max" }));
      render(<MergeInspector />);
      // `sonnet` has no model_efforts key: the global offer applies — `max` is a
      // supported choice, not a warning.
      expect(await screen.findByTestId("merge-effort-option-max")).toHaveAttribute(
        "aria-checked",
        "true",
      );
      expect(screen.queryByTestId("merge-effort-unsupported")).toBeNull();
    });

    it("a harness unknown to the catalogue preserves the pass-through UI (no warning)", async () => {
      setStoreState(makeMergeNode({ pin_harness: "mystery", effort: "turbo" }));
      render(<MergeInspector />);
      expect(screen.getByTestId("merge-harness-resolved")).toHaveTextContent("mystery");
      await waitFor(() => expect(vi.mocked(fetchSettings)).toHaveBeenCalled());
      const extra = screen.getByTestId("merge-effort-option-passthrough");
      expect(extra).toHaveTextContent("turbo");
      expect(extra).toHaveAttribute("aria-checked", "true");
      expect(extra).not.toHaveAttribute("data-unsupported");
      expect(screen.queryByTestId("merge-effort-unsupported")).toBeNull();
    });
  });
});
