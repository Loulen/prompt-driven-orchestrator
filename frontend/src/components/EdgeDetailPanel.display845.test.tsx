import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, beforeEach } from "vitest";
import EdgeDetailPanel from "./EdgeDetailPanel";
import { useEditStore } from "../stores/editStore";
import { pipelineToYamlObject } from "../lib/serializePipeline";
import type { PipelineDef, NodeDef, EdgeDef } from "../types";

// #845 — the Display section: whether the carried outputs are NAMED on the
// canvas, and whether the edge passes over or under the node cards. Both are
// layout, and the section says so.

function source(outputs: string[]): NodeDef {
  return {
    id: "design",
    name: "design",
    type: "agent",
    inputs: [],
    outputs: outputs.map((name) => ({ name, repeated: false, side: "right" as const })),
    interactive: false,
    view: { x: 0, y: 0 },
  };
}

function target(): NodeDef {
  return {
    id: "orch",
    name: "orchestrator",
    type: "agent",
    inputs: [],
    outputs: [{ name: "out", repeated: false, side: "right" }],
    interactive: false,
    view: { x: 300, y: 0 },
  };
}

function seedEdge(edge: EdgeDef, declared: string[] = ["out", "spec"]) {
  const pipeline: PipelineDef = {
    name: "edge-845",
    version: "1.0",
    variables: {},
    nodes: [source(declared), target()],
    edges: [edge],
  };
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
    selection: { kind: "edge", id: null, edgeIndex: 0 },
  });
}

const plainEdge: EdgeDef = {
  source: { node: "design", port: "out" },
  target: { node: "orch", port: "out" },
};

function currentEdge(): EdgeDef {
  return useEditStore.getState().openTabs[0].pipeline.edges[0];
}

function emittedEdge(): Record<string, unknown> {
  const obj = pipelineToYamlObject(useEditStore.getState().openTabs[0].pipeline);
  return (obj.edges as Record<string, unknown>[])[0];
}

beforeEach(() => {
  useEditStore.setState({
    openTabs: [],
    activeTabId: null,
    selection: { kind: "none", id: null },
  });
});

describe("Display section (#845)", () => {
  it("shows both switches and says the whole section is layout", () => {
    seedEdge(plainEdge);
    render(<EdgeDetailPanel />);

    expect(screen.getByTestId("display-section")).toBeInTheDocument();
    expect(screen.getByTestId("toggle-output-labels")).toBeInTheDocument();
    expect(screen.getByTestId("toggle-under-nodes")).toBeInTheDocument();
    expect(screen.getByTestId("display-layout-note")).toHaveTextContent(
      "Layout only — saved in the file, ignored by the semantic diff.",
    );
  });

  it("sits between Routing and Runtime", () => {
    seedEdge(plainEdge);
    render(<EdgeDetailPanel />);
    const text = screen.getByTestId("edge-detail-panel").textContent ?? "";
    expect(text.indexOf("Routing")).toBeLessThan(text.indexOf("Display"));
    expect(text.indexOf("Display")).toBeLessThan(text.indexOf("Runtime"));
  });

  it("starts ON and marked '· default' when the source declares two outputs", () => {
    seedEdge(plainEdge, ["out", "spec"]);
    render(<EdgeDetailPanel />);

    expect(screen.getByTestId("toggle-output-labels")).toHaveAttribute("aria-checked", "true");
    expect(screen.getByTestId("toggle-output-labels-note")).toHaveTextContent("· default");
  });

  it("starts OFF and marked '· default' when the source declares a single output", () => {
    seedEdge(plainEdge, ["out"]);
    render(<EdgeDetailPanel />);

    expect(screen.getByTestId("toggle-output-labels")).toHaveAttribute("aria-checked", "false");
    expect(screen.getByTestId("toggle-output-labels-note")).toHaveTextContent("· default");
  });

  it("drops the '· default' mark once the author decides, and writes the value", () => {
    seedEdge(plainEdge, ["out", "spec"]);
    render(<EdgeDetailPanel />);

    fireEvent.click(screen.getByTestId("toggle-output-labels"));

    expect(currentEdge().show_output_labels).toBe(false);
    expect(screen.getByTestId("toggle-output-labels")).toHaveAttribute("aria-checked", "false");
    expect(screen.queryByTestId("toggle-output-labels-note")).toBeNull();
  });

  it("records a decision that AGREES with the default, so a new output cannot flip it back", () => {
    // Turned on by hand on a single-output node: adding a second output later
    // must not be able to claim the value was never chosen.
    seedEdge(plainEdge, ["out"]);
    render(<EdgeDetailPanel />);

    fireEvent.click(screen.getByTestId("toggle-output-labels"));

    expect(currentEdge().show_output_labels).toBe(true);
    expect(screen.queryByTestId("toggle-output-labels-note")).toBeNull();
  });

  it("draws above the nodes until the switch says otherwise", () => {
    seedEdge(plainEdge);
    render(<EdgeDetailPanel />);

    expect(screen.getByTestId("toggle-under-nodes")).toHaveAttribute("aria-checked", "false");
    fireEvent.click(screen.getByTestId("toggle-under-nodes"));
    expect(currentEdge().below_nodes).toBe(true);
    expect(screen.getByTestId("toggle-under-nodes")).toHaveAttribute("aria-checked", "true");
  });

  it("emits nothing for an untouched edge, and only the non-defaults after an edit", () => {
    seedEdge(plainEdge, ["out", "spec"]);
    const { rerender } = render(<EdgeDetailPanel />);
    expect(emittedEdge()).not.toHaveProperty("show_output_labels");
    expect(emittedEdge()).not.toHaveProperty("below_nodes");

    fireEvent.click(screen.getByTestId("toggle-under-nodes"));
    rerender(<EdgeDetailPanel />);
    fireEvent.click(screen.getByTestId("toggle-output-labels"));

    expect(emittedEdge()).toMatchObject({ below_nodes: true, show_output_labels: false });
  });

  it("switching the draw order back off stops emitting it", () => {
    seedEdge({ ...plainEdge, below_nodes: true });
    render(<EdgeDetailPanel />);

    fireEvent.click(screen.getByTestId("toggle-under-nodes"));

    expect(currentEdge().below_nodes).toBe(false);
    expect(emittedEdge()).not.toHaveProperty("below_nodes");
  });
});
