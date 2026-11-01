import { render, screen, fireEvent, within } from "@testing-library/react";
import { describe, it, expect, beforeEach } from "vitest";
import EdgeDetailPanel from "./EdgeDetailPanel";
import { useEditStore } from "../store/editStore";
import { pipelineToYamlObject } from "../../lib/serializePipeline";
import type { EdgeDef } from "../../shared/generated/EdgeDef";
import type { EditorNodeDef, EditorPipelineDef } from "../model/pipeline";
import { pickOption, selectOptions, selectedLabel } from "../../test/kitSelect";

function reviewer(): EditorNodeDef {
  return {
    id: "reviewer",
    name: "reviewer",
    type: "agent",
    inputs: [{ name: "task" }],
    outputs: [
      {
        name: "verdict",
        frontmatter: {
          verdict: { type: "enum", allowed: ["PASS", "FAIL", "NEEDS_WORK"] },
          is_blocking: { type: "bool" },
        },
      },
    ],
    interactive: false,
    view: { x: 0, y: 0 },
  };
}

function impl(): EditorNodeDef {
  return {
    id: "impl",
    name: "implementer",
    type: "agent",
    inputs: [{ name: "review" }],
    outputs: [{ name: "diff" }],
    interactive: false,
    view: { x: 200, y: 0 },
  };
}

function makePipeline(edges: EdgeDef[]): EditorPipelineDef {
  return {
    name: "edge-test",
    version: "1.0",
    variables: {},
    nodes: [reviewer(), impl()],
    edges,
  };
}

function seedEdge(edge: EdgeDef, edgeIndex = 0) {
  useEditStore.setState({
    openTabs: [
      {
        id: "tab1",
        scope: "repo",
        pipeline: makePipeline([edge]),
        prompts: {},
        diagnostics: [],
        dirty: false,
        externalDirty: false,
      },
    ],
    activeTabId: "tab1",
    selection: { kind: "edge", id: null, edgeIndex },
  });
}

const baseEdge: EdgeDef = {
  source: { node: "reviewer", port: "verdict" },
  target: { node: "impl", port: "review" },
};

beforeEach(() => {
  useEditStore.setState({
    openTabs: [],
    activeTabId: null,
    selection: { kind: "none", id: null },
  });
});

describe("EdgeDetailPanel", () => {
  it("shows the route source.port -> target", () => {
    seedEdge(baseEdge);
    render(<EdgeDetailPanel />);
    const head = screen.getByTestId("edge-detail-panel");
    expect(head).toHaveTextContent("reviewer");
    expect(head).toHaveTextContent("verdict");
    expect(head).toHaveTextContent("implementer");
  });

  it("renders nothing when no edge is selected", () => {
    useEditStore.setState({ selection: { kind: "none", id: null } });
    const { container } = render(<EdgeDetailPanel />);
    expect(container).toBeEmptyDOMElement();
  });

  it("authors a when: clause via field/op/value and writes it to the edge", () => {
    seedEdge(baseEdge);
    render(<EdgeDetailPanel />);

    fireEvent.click(screen.getByTestId("add-condition"));

    pickOption(screen.getByTestId("field-dropdown"), "verdict");
    pickOption(screen.getByTestId("op-dropdown"), "=");
    // enum field exposes its allowed values as a dropdown
    pickOption(screen.getByTestId("value-dropdown"), "FAIL");

    const edge = useEditStore.getState().openTabs[0].pipeline.edges[0];
    expect(edge.when).toEqual({ verdict: { eq: "FAIL" } });
  });

  it("renders a true/false toggle for a boolean field and writes a canonical boolean", () => {
    seedEdge({ ...baseEdge, when: { is_blocking: { eq: false } } });
    render(<EdgeDetailPanel />);

    // No free-text value input for a bool field; a toggle is shown instead.
    const row = screen.getByTestId("condition-row");
    expect(within(row).queryByTestId("value-input")).toBeNull();
    const toggleTrue = within(within(row).getByTestId("bool-toggle")).getByRole("radio", { name: "true" });

    fireEvent.click(toggleTrue);

    const edge = useEditStore.getState().openTabs[0].pipeline.edges[0];
    expect(edge.when).toEqual({ is_blocking: { eq: true } });
    // canonical boolean, not the string "true"
    expect(typeof (edge.when!.is_blocking as Record<string, unknown>).eq).toBe("boolean");
  });

  // #456: the operator dropdown is typed. A bool has no useful ordering, so
  // offering `<`/`≤`/`>`/`≥` (and the list operators) let `is_blocking >= true`
  // be authored and saved.
  describe("operator set is typed (#456)", () => {
    it("offers only = and ≠ for a boolean field", () => {
      seedEdge({ ...baseEdge, when: { is_blocking: { eq: true } } });
      render(<EdgeDetailPanel />);
      expect(selectOptions(screen.getByTestId("op-dropdown")).map((o) => o.label)).toEqual(["=", "≠"]);
    });

    it("keeps the full operator set for iter and for an enum field", () => {
      seedEdge({ ...baseEdge, when: { iter: { gte: 3 } } });
      render(<EdgeDetailPanel />);
      expect(selectOptions(screen.getByTestId("op-dropdown")).map((o) => o.label)).toEqual([
        "=", "≠", "<", "≤", ">", "≥", "in", "not in",
      ]);
    });

    it("clamps the operator to eq when the field is switched to a bool", () => {
      seedEdge({ ...baseEdge, when: { iter: { gte: 3 } } });
      render(<EdgeDetailPanel />);

      pickOption(screen.getByTestId("field-dropdown"), "is_blocking");

      const edge = useEditStore.getState().openTabs[0].pipeline.edges[0];
      // `gte` would have survived the field change and written `is_blocking >= true`.
      expect(edge.when).toEqual({ is_blocking: { eq: true } });
    });

    /**
     * A pipeline authored before the fix (or by hand) can carry `is_blocking:
     * {gte: true}`. Dropping `gte` from the options would leave the `<select>`
     * DISPLAYING `=` while the row still holds `gte` — the same
     * shows-one-thing-sends-another trap as #454. The panel already keeps that
     * escape hatch for an unknown field; the operator needs it too.
     */
    it("still renders an out-of-range operator already present on the edge", () => {
      seedEdge({ ...baseEdge, when: { is_blocking: { gte: true } } });
      render(<EdgeDetailPanel />);
      const opSel = screen.getByTestId("op-dropdown");
      expect(selectedLabel(opSel)).toBe("≥");
      expect(selectOptions(opSel).map((o) => o.label)).toContain("≥");
    });
  });

  it("adds a second condition without touching the value of the first", () => {
    seedEdge({ ...baseEdge, when: { verdict: { eq: "FAIL" } } });
    render(<EdgeDetailPanel />);
    fireEvent.click(screen.getByTestId("add-condition"));
    expect(screen.getAllByTestId("condition-row")).toHaveLength(2);
    const edge = useEditStore.getState().openTabs[0].pipeline.edges[0];
    expect((edge.when as Record<string, Record<string, unknown>>).verdict).toEqual({ eq: "FAIL" });
  });

  // A bool condition kept its type only while it was the last row committed: the
  // next commit (an added or edited condition) wrote it back as the string "true",
  // which the runtime never matches against a frontmatter `true`.
  it("keeps a bool condition a bool when another condition is added and edited", () => {
    seedEdge(baseEdge);
    render(<EdgeDetailPanel />);

    fireEvent.click(screen.getByTestId("add-condition"));
    pickOption(screen.getByTestId("field-dropdown"), "is_blocking");
    fireEvent.click(screen.getByTestId("add-condition"));
    pickOption(screen.getAllByTestId("field-dropdown")[1], "verdict");
    pickOption(screen.getAllByTestId("value-dropdown")[0], "FAIL");

    let when = useEditStore.getState().openTabs[0].pipeline.edges[0].when;
    expect(when).toEqual({ is_blocking: { eq: true }, verdict: { eq: "FAIL" } });

    pickOption(screen.getAllByTestId("value-dropdown")[0], "PASS");
    when = useEditStore.getState().openTabs[0].pipeline.edges[0].when;
    expect(when).toEqual({ is_blocking: { eq: true }, verdict: { eq: "PASS" } });
  });

  it("keeps numbers and numeric-looking strings typed when another condition is added", () => {
    seedEdge({ ...baseEdge, when: { iter: { gte: 2 }, verdict: { eq: "42" }, is_blocking: { eq: false } } });
    render(<EdgeDetailPanel />);
    fireEvent.click(screen.getByTestId("add-condition"));
    const when = useEditStore.getState().openTabs[0].pipeline.edges[0].when as Record<string, Record<string, unknown>>;
    expect(when.iter.gte).toBe(2);
    expect(when.verdict.eq).toBe("42");
    expect(when.is_blocking).toEqual({ eq: false });
  });

  it("keeps a numeric-looking string a string when only its operator changes", () => {
    seedEdge({ ...baseEdge, when: { iter: { eq: "42" } } });
    render(<EdgeDetailPanel />);
    pickOption(screen.getByTestId("op-dropdown"), "≠");
    const when = useEditStore.getState().openTabs[0].pipeline.edges[0].when as Record<string, Record<string, unknown>>;
    expect(when.iter).toEqual({ neq: "42" });
  });

  it("keeps a bool on a field no output declares a bool when only its operator changes", () => {
    seedEdge({ ...baseEdge, when: { flag: { eq: true } } });
    render(<EdgeDetailPanel />);
    pickOption(screen.getByTestId("op-dropdown"), "≠");
    const when = useEditStore.getState().openTabs[0].pipeline.edges[0].when as Record<string, Record<string, unknown>>;
    expect(when.flag).toEqual({ neq: true });
  });

  it("writes an enum value that reads as a number as a string", () => {
    seedEdge({ ...baseEdge, when: { level: { eq: "1" } } });
    const tab = useEditStore.getState().openTabs[0];
    const nodes = tab.pipeline.nodes.map((n) =>
      n.id === "reviewer"
        ? { ...n, outputs: [{ name: "verdict", frontmatter: { level: { type: "enum", allowed: ["1", "2"] } } }] }
        : n,
    );
    useEditStore.setState({ openTabs: [{ ...tab, pipeline: { ...tab.pipeline, nodes } }] });
    render(<EdgeDetailPanel />);
    pickOption(screen.getByTestId("value-dropdown"), "2");
    const when = useEditStore.getState().openTabs[0].pipeline.edges[0].when as Record<string, Record<string, unknown>>;
    expect(when.level).toEqual({ eq: "2" });
  });

  it("moves a row onto a held field with another operator, keeping both values", () => {
    seedEdge({ ...baseEdge, when: { verdict: { eq: "FAIL" }, iter: { gte: 2 } } });
    render(<EdgeDetailPanel />);
    pickOption(screen.getAllByTestId("field-dropdown")[1], "verdict");
    const when = useEditStore.getState().openTabs[0].pipeline.edges[0].when as Record<string, Record<string, unknown>>;
    expect(when.verdict.eq).toBe("FAIL");
    expect(Object.keys(when.verdict)).toHaveLength(2);
  });

  it("shows the conditions in the order they were added", () => {
    seedEdge(baseEdge);
    render(<EdgeDetailPanel />);
    const shown = () =>
      screen.getAllByTestId("condition-row").map((row) => {
        const field = selectedLabel(within(row).getByTestId("field-dropdown"));
        const op = selectedLabel(within(row).getByTestId("op-dropdown"));
        return `${field} ${op}`;
      });
    const seen: string[] = [];
    for (let i = 0; i < 4; i++) {
      fireEvent.click(screen.getByTestId("add-condition"));
      const now = shown();
      expect(now.slice(0, seen.length)).toEqual(seen);
      seen.push(now[now.length - 1]);
    }
    // The fourth reuses a field: the clause groups it with the first.
    expect(new Set(seen.map((s) => s.split(" ")[0])).size).toBeLessThan(4);
  });

  describe("Conditions section", () => {
    it("is titled « Conditions », with no Fields section", () => {
      seedEdge(baseEdge);
      render(<EdgeDetailPanel />);
      const panel = screen.getByTestId("edge-detail-panel");
      expect(within(panel).getByText("Conditions")).toBeInTheDocument();
      expect(within(panel).queryByText("When")).toBeNull();
      expect(within(panel).queryByText("Fields")).toBeNull();
    });

    it("offers « Add condition » under « Always » when the edge has none", () => {
      seedEdge(baseEdge);
      render(<EdgeDetailPanel />);
      const editor = screen.getByTestId("when-editor");
      const add = within(editor).getByRole("button", { name: "Add condition" });
      expect(add).toBeEnabled();
      const always = within(editor).getByText("Always");
      expect(always.compareDocumentPosition(add) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    });

    it("keeps « Add condition » under the last condition row", () => {
      seedEdge({ ...baseEdge, when: { verdict: { eq: "FAIL" }, iter: { gte: 2 } } });
      render(<EdgeDetailPanel />);
      const rows = screen.getAllByTestId("condition-row");
      const add = screen.getByRole("button", { name: "Add condition" });
      expect(rows[rows.length - 1].compareDocumentPosition(add) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
      fireEvent.click(add);
      const after = screen.getAllByTestId("condition-row");
      expect(after).toHaveLength(3);
      expect(after[2].compareDocumentPosition(screen.getByRole("button", { name: "Add condition" })) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    });

    it("disables « Add condition » with its reason when no field/operator pair is free", () => {
      // A port with no frontmatter and no variable leaves `iter` alone: eight operators.
      const plain: EdgeDef = { source: { node: "impl", port: "diff" }, target: { node: "reviewer", port: "task" } };
      seedEdge({
        ...plain,
        when: { iter: { eq: 1, neq: 2, lt: 3, lte: 4, gt: 5, gte: 6, in: [1], not_in: [2] } },
      });
      render(<EdgeDetailPanel />);
      const add = screen.getByRole("button", { name: "Add condition" });
      expect(add).toBeDisabled();
      expect(screen.getByTestId("add-condition-why")).toHaveTextContent(/already has a condition/);
      expect(add.parentElement).toHaveAttribute("title", expect.stringMatching(/already has a condition/));
    });

    it("groups the field options by origin, each with its type and allowed values", () => {
      const pipeline = makePipeline([{ ...baseEdge, when: { verdict: { eq: "FAIL" } } }]);
      pipeline.variables = { max_iter: { type: "int", default: 5 } };
      useEditStore.setState({
        openTabs: [{ id: "tab1", scope: "repo", pipeline, prompts: {}, diagnostics: [], dirty: false, externalDirty: false }],
        activeTabId: "tab1",
        selection: { kind: "edge", id: null, edgeIndex: 0 },
      });
      render(<EdgeDetailPanel />);
      expect(selectOptions(screen.getByTestId("field-dropdown"))).toEqual([
        { label: "verdict", hint: "enum · PASS | FAIL | NEEDS_WORK", group: "Output · verdict" },
        { label: "is_blocking", hint: "bool", group: "Output · verdict" },
        { label: "iter", hint: "int · loop iteration", group: "Loop" },
        { label: "$max_iter", hint: "int · default 5", group: "Pipeline variables" },
      ]);
    });

    it("labels the columns of the condition grid", () => {
      seedEdge({ ...baseEdge, when: { verdict: { eq: "FAIL" } } });
      render(<EdgeDetailPanel />);
      expect(screen.getByTestId("condition-grid-head")).toHaveTextContent("OutputFieldOpValue");
    });
  });

  it("offers iter as a selectable condition field", () => {
    seedEdge(baseEdge);
    render(<EdgeDetailPanel />);
    fireEvent.click(screen.getByTestId("add-condition"));
    const options = selectOptions(screen.getByTestId("field-dropdown")).map((o) => o.label);
    expect(options).toContain("iter");
  });

  // #179: default (else) edge affordance — fires iff no sibling matched.
  describe("default (else) toggle", () => {
    it("renders the toggle off for a plain/guarded edge and shows the when editor", () => {
      seedEdge(baseEdge);
      render(<EdgeDetailPanel />);
      const toggle = screen.getByTestId("else-toggle");
      expect(toggle).toHaveAttribute("aria-checked", "false");
      expect(screen.getByTestId("when-editor")).toBeInTheDocument();
      expect(screen.queryByTestId("else-active-note")).toBeNull();
    });

    it("toggling it on marks the edge else:true and clears any when rows", () => {
      seedEdge({ ...baseEdge, when: { verdict: { eq: "FAIL" } } });
      render(<EdgeDetailPanel />);

      fireEvent.click(screen.getByTestId("else-toggle"));

      const edge = useEditStore.getState().openTabs[0].pipeline.edges[0];
      expect(edge.else).toBe(true);
      expect(edge.when == null || Object.keys(edge.when).length === 0).toBe(true);
    });

    it("an else edge reads the toggle on and suppresses the predicate editor", () => {
      seedEdge({ ...baseEdge, else: true });
      render(<EdgeDetailPanel />);

      expect(screen.getByTestId("else-toggle")).toHaveAttribute("aria-checked", "true");
      // No predicate authoring surface for a default edge.
      expect(screen.queryByTestId("when-editor")).toBeNull();
      expect(screen.queryByTestId("add-condition")).toBeNull();
      expect(screen.getByTestId("else-active-note")).toBeInTheDocument();
    });

    it("toggling it off clears else, restoring the predicate editor", () => {
      seedEdge({ ...baseEdge, else: true });
      render(<EdgeDetailPanel />);

      fireEvent.click(screen.getByTestId("else-toggle"));

      const edge = useEditStore.getState().openTabs[0].pipeline.edges[0];
      expect(edge.else).toBeFalsy();
      // The predicate editor comes back once the edge is no longer a default.
      expect(screen.getByTestId("when-editor")).toBeInTheDocument();
    });

    it("clears else when a when: condition is authored (mutually exclusive)", () => {
      // The auto-clear invariant in commitRows still holds: a guarded edge that
      // somehow carried else loses it the moment a predicate is committed.
      seedEdge(baseEdge);
      render(<EdgeDetailPanel />);
      fireEvent.click(screen.getByTestId("add-condition"));
      const edge = useEditStore.getState().openTabs[0].pipeline.edges[0];
      expect(edge.when).not.toBeNull();
      expect(edge.else).toBeFalsy();
    });

    it("an else edge serializes to `else: true` and round-trips through encode", () => {
      seedEdge({ ...baseEdge, else: true });
      render(<EdgeDetailPanel />);

      const pipeline = useEditStore.getState().openTabs[0].pipeline;
      const yaml = pipelineToYamlObject(pipeline);
      const edges = yaml.edges as Record<string, unknown>[];
      expect(edges[0].else).toBe(true);
      // A default edge carries no predicate in the persisted form.
      expect(edges[0].when).toBeUndefined();
    });

    it("a toggled-on edge serializes to `else: true` with no when clause", () => {
      seedEdge({ ...baseEdge, when: { verdict: { eq: "FAIL" } } });
      render(<EdgeDetailPanel />);

      fireEvent.click(screen.getByTestId("else-toggle"));

      const pipeline = useEditStore.getState().openTabs[0].pipeline;
      const edges = pipelineToYamlObject(pipeline).edges as Record<string, unknown>[];
      expect(edges[0].else).toBe(true);
      expect(edges[0].when).toBeUndefined();
    });
  });

  it("deletes a condition row, clearing the when clause", () => {
    seedEdge({ ...baseEdge, when: { verdict: { eq: "FAIL" } } });
    render(<EdgeDetailPanel />);
    fireEvent.click(screen.getByTestId("delete-condition"));
    const edge = useEditStore.getState().openTabs[0].pipeline.edges[0];
    expect(edge.when == null || Object.keys(edge.when).length === 0).toBe(true);
  });

  // #154: the per-edge routing reset lives in the detail panel (design screen 14).
  describe("Routing section", () => {
    it("reads 'Automatic' for an auto edge and offers no reset", () => {
      seedEdge(baseEdge);
      render(<EdgeDetailPanel />);
      const routing = screen.getByTestId("edge-routing");
      expect(routing).toHaveTextContent(/Automatic/i);
      expect(screen.queryByTestId("reroute-auto")).toBeNull();
    });

    it("reads 'Manually pinned' with the waypoint count for a manual edge", () => {
      seedEdge({
        ...baseEdge,
        mode: "manual",
        waypoints: [
          { x: 10, y: 20 },
          { x: 10, y: 80 },
        ],
      });
      render(<EdgeDetailPanel />);
      const routing = screen.getByTestId("edge-routing");
      expect(routing).toHaveTextContent(/Manually pinned/i);
      expect(routing).toHaveTextContent("2");
    });

    it("'Re-route automatically' resets the edge to auto and clears waypoints", () => {
      seedEdge({
        ...baseEdge,
        mode: "manual",
        waypoints: [
          { x: 10, y: 20 },
          { x: 10, y: 80 },
        ],
      });
      render(<EdgeDetailPanel />);
      fireEvent.click(screen.getByTestId("reroute-auto"));
      const edge = useEditStore.getState().openTabs[0].pipeline.edges[0];
      expect(edge.mode === "auto" || edge.mode == null).toBe(true);
      expect(edge.waypoints == null || edge.waypoints.length === 0).toBe(true);
    });
  });
});
