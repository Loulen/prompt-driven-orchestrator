import { render, screen, fireEvent, within } from "@testing-library/react";
import { describe, it, expect, beforeEach } from "vitest";
import EdgeDetailPanel from "./EdgeDetailPanel";
import { useEditStore, pipelineToYamlObject } from "../stores/editStore";
import type { PipelineDef, NodeDef, EdgeDef } from "../types";

function reviewer(): NodeDef {
  return {
    id: "reviewer",
    name: "reviewer",
    type: "doc-only",
    inputs: [{ name: "task", repeated: false, side: "left" }],
    outputs: [
      {
        name: "verdict",
        repeated: false,
        side: "right",
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

function impl(): NodeDef {
  return {
    id: "impl",
    name: "implementer",
    type: "code-mutating",
    inputs: [{ name: "review", repeated: false, side: "left" }],
    outputs: [{ name: "diff", repeated: false, side: "right" }],
    interactive: false,
    view: { x: 200, y: 0 },
  };
}

function makePipeline(edges: EdgeDef[]): PipelineDef {
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

    const fieldSel = screen.getByTestId("field-dropdown");
    fireEvent.change(fieldSel, { target: { value: "verdict" } });
    const opSel = screen.getByTestId("op-dropdown");
    fireEvent.change(opSel, { target: { value: "eq" } });
    // enum field exposes its allowed values as a dropdown
    const valueSel = screen.getByTestId("value-dropdown");
    fireEvent.change(valueSel, { target: { value: "FAIL" } });

    const edge = useEditStore.getState().openTabs[0].pipeline.edges[0];
    expect(edge.when).toEqual({ verdict: { eq: "FAIL" } });
  });

  it("renders a true/false toggle for a boolean field and writes a canonical boolean", () => {
    seedEdge({ ...baseEdge, when: { is_blocking: { eq: false } } });
    render(<EdgeDetailPanel />);

    // No free-text value input for a bool field; a toggle is shown instead.
    const row = screen.getByTestId("condition-row");
    expect(within(row).queryByTestId("value-input")).toBeNull();
    const toggleTrue = within(row).getByTestId("bool-true");

    fireEvent.click(toggleTrue);

    const edge = useEditStore.getState().openTabs[0].pipeline.edges[0];
    expect(edge.when).toEqual({ is_blocking: { eq: true } });
    // canonical boolean, not the string "true"
    expect(typeof (edge.when!.is_blocking as Record<string, unknown>).eq).toBe("boolean");
  });

  it("offers iter as a selectable condition field", () => {
    seedEdge(baseEdge);
    render(<EdgeDetailPanel />);
    fireEvent.click(screen.getByTestId("add-condition"));
    const fieldSel = screen.getByTestId("field-dropdown") as HTMLSelectElement;
    const options = Array.from(fieldSel.options).map((o) => o.value);
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
