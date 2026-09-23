import { render, screen, fireEvent, within } from "@testing-library/react";
import { describe, it, expect, beforeEach } from "vitest";
import EdgeDetailPanel from "./EdgeDetailPanel";
import { useEditStore } from "../stores/editStore";
import { pipelineToYamlObject } from "../lib/serializePipeline";
import type { PipelineDef, NodeDef, EdgeDef } from "../types";

// #843 — the Outputs section: an edge carries one or more outputs of its source
// node (ADR-0073), chosen here, and each condition row names the output it reads.

function design(): NodeDef {
  return {
    id: "design",
    name: "design",
    type: "agent",
    inputs: [{ name: "brief", repeated: false, side: "left" }],
    outputs: [
      {
        name: "out",
        repeated: false,
        side: "right",
        frontmatter: { has_design_work: { type: "bool" } },
      },
      {
        name: "spec",
        repeated: false,
        side: "right",
        frontmatter: { ready: { type: "bool" } },
      },
      { name: "notes", repeated: false, side: "right" },
    ],
    interactive: false,
    view: { x: 0, y: 0 },
  };
}

function orchestrator(): NodeDef {
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

function seedEdge(edge: EdgeDef) {
  const pipeline: PipelineDef = {
    name: "edge-843",
    version: "1.0",
    variables: {},
    nodes: [design(), orchestrator()],
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

const singlePort: EdgeDef = {
  source: { node: "design", port: "out" },
  target: { node: "orch", port: "out" },
};

function currentEdge(): EdgeDef {
  return useEditStore.getState().openTabs[0].pipeline.edges[0];
}

beforeEach(() => {
  useEditStore.setState({
    openTabs: [],
    activeTabId: null,
    selection: { kind: "none", id: null },
  });
});

describe("Outputs section (#843)", () => {
  it("lists one checkbox per declared output, ticked for the carried ones", () => {
    seedEdge(singlePort);
    render(<EdgeDetailPanel />);

    const section = screen.getByTestId("outputs-section");
    for (const port of ["out", "spec", "notes"]) {
      expect(within(section).getByTestId(`output-checkbox-${port}`)).toBeInTheDocument();
    }
    const checkbox = (port: string) =>
      within(screen.getByTestId(`output-checkbox-${port}`)).getByRole("checkbox");
    expect(checkbox("out")).toBeChecked();
    expect(checkbox("spec")).not.toBeChecked();
    expect(checkbox("notes")).not.toBeChecked();
  });

  it("is the FIRST section of the panel, before When", () => {
    seedEdge(singlePort);
    render(<EdgeDetailPanel />);
    const text = screen.getByTestId("edge-detail-panel").textContent ?? "";
    expect(text.indexOf("Outputs")).toBeGreaterThanOrEqual(0);
    expect(text.indexOf("Outputs")).toBeLessThan(text.indexOf("When"));
  });

  it("ticking a second output makes the edge carry both, in declaration order", () => {
    seedEdge(singlePort);
    render(<EdgeDetailPanel />);

    fireEvent.click(within(screen.getByTestId("output-checkbox-spec")).getByRole("checkbox"));

    expect(currentEdge().source).toEqual({ node: "design", ports: ["out", "spec"] });
  });

  it("ticking an output declared BEFORE the carried one still lands in declaration order", () => {
    seedEdge({ ...singlePort, source: { node: "design", port: "notes" } });
    render(<EdgeDetailPanel />);

    fireEvent.click(within(screen.getByTestId("output-checkbox-out")).getByRole("checkbox"));

    expect(currentEdge().source).toEqual({ node: "design", ports: ["out", "notes"] });
  });

  it("unticking back to one output restores the single-port shape", () => {
    // The backward-compatibility contract: the file must not be able to tell an
    // edge that lost its second output from one that never had one.
    seedEdge({ ...singlePort, source: { node: "design", ports: ["out", "spec"] } });
    render(<EdgeDetailPanel />);

    fireEvent.click(within(screen.getByTestId("output-checkbox-spec")).getByRole("checkbox"));

    expect(currentEdge().source).toEqual({ node: "design", port: "out" });
  });

  it("refuses to untick the last carried output, and says why", () => {
    seedEdge(singlePort);
    render(<EdgeDetailPanel />);

    fireEvent.click(within(screen.getByTestId("output-checkbox-out")).getByRole("checkbox"));

    expect(currentEdge().source).toEqual({ node: "design", port: "out" });
    expect(screen.getByTestId("outputs-last-hint")).toHaveTextContent(
      /at least one output/i,
    );
  });

  it("explains the one-edge-one-firing rule once two outputs are carried", () => {
    seedEdge({ ...singlePort, source: { node: "design", ports: ["out", "spec"] } });
    render(<EdgeDetailPanel />);
    expect(screen.getByTestId("outputs-multi-hint")).toHaveTextContent(
      /one emergent input per carried port/i,
    );
    expect(screen.getByTestId("outputs-multi-hint")).toHaveTextContent("orchestrator");
  });

  it("names every carried output in the route header", () => {
    seedEdge({ ...singlePort, source: { node: "design", ports: ["out", "spec"] } });
    render(<EdgeDetailPanel />);
    expect(screen.getByTestId("edge-route-ports")).toHaveTextContent(".out+spec");
  });

  it("keeps a port the node no longer declares visible and ticked", () => {
    // The panel reports what the YAML says; silently dropping a since-renamed
    // output would look like the edge changed on its own.
    seedEdge({ ...singlePort, source: { node: "design", ports: ["out", "gone"] } });
    render(<EdgeDetailPanel />);
    const checkbox = within(screen.getByTestId("output-checkbox-gone")).getByRole("checkbox");
    expect(checkbox).toBeChecked();
  });
});

describe("condition rows read a designated output (#843)", () => {
  it("offers only the carried outputs in the row's leading dropdown", () => {
    seedEdge({
      ...singlePort,
      source: { node: "design", ports: ["out", "spec"] },
      when: { "out.has_design_work": { eq: true } },
    });
    render(<EdgeDetailPanel />);
    const sel = screen.getByTestId("condition-port-dropdown") as HTMLSelectElement;
    expect(Array.from(sel.options).map((o) => o.value)).toEqual(["out", "spec"]);
    expect(sel.value).toBe("out");
  });

  it("shows the dropdown on a single-port edge too, so rows keep their shape", () => {
    seedEdge({ ...singlePort, when: { has_design_work: { eq: true } } });
    render(<EdgeDetailPanel />);
    const sel = screen.getByTestId("condition-port-dropdown") as HTMLSelectElement;
    expect(sel.value).toBe("out");
    expect(Array.from(sel.options).map((o) => o.value)).toEqual(["out"]);
  });

  it("resolves the field list from the row's output, not the edge's first port", () => {
    seedEdge({
      ...singlePort,
      source: { node: "design", ports: ["out", "spec"] },
      when: { "spec.ready": { eq: true } },
    });
    render(<EdgeDetailPanel />);
    const fields = screen.getByTestId("field-dropdown") as HTMLSelectElement;
    const names = Array.from(fields.options).map((o) => o.value);
    expect(names).toContain("ready");
    expect(names).not.toContain("has_design_work");
  });

  it("switching the row's output rewrites the clause key and re-picks the property", () => {
    seedEdge({
      ...singlePort,
      source: { node: "design", ports: ["out", "spec"] },
      when: { "out.has_design_work": { eq: true } },
    });
    render(<EdgeDetailPanel />);

    fireEvent.change(screen.getByTestId("condition-port-dropdown"), {
      target: { value: "spec" },
    });

    // `has_design_work` does not exist on `spec`; keeping it would leave the
    // dropdown showing a property the port does not declare.
    expect(currentEdge().when).toEqual({ "spec.ready": { eq: true } });
  });

  it("a new condition on a multi-port edge defaults to `out` and writes a qualified key", () => {
    seedEdge({ ...singlePort, source: { node: "design", ports: ["spec", "out"] } });
    render(<EdgeDetailPanel />);

    fireEvent.click(screen.getByTestId("add-condition"));

    expect(
      (screen.getByTestId("condition-port-dropdown") as HTMLSelectElement).value,
    ).toBe("out");
    expect(Object.keys(currentEdge().when ?? {})[0]).toBe("out.has_design_work");
  });

  it("a new condition on a single-port edge writes the bare key (unchanged YAML)", () => {
    seedEdge(singlePort);
    render(<EdgeDetailPanel />);

    fireEvent.click(screen.getByTestId("add-condition"));

    expect(currentEdge().when).toEqual({ has_design_work: { eq: true } });
  });

  it("unticking an output re-points the rows that read it (ADR-0073 §3)", () => {
    seedEdge({
      ...singlePort,
      source: { node: "design", ports: ["out", "spec"] },
      when: { "spec.ready": { eq: true } },
    });
    render(<EdgeDetailPanel />);

    fireEvent.click(within(screen.getByTestId("output-checkbox-spec")).getByRole("checkbox"));

    // Back to one port: the qualifier goes with it, and the row now reads `out`.
    expect(currentEdge().source).toEqual({ node: "design", port: "out" });
    expect(currentEdge().when).toEqual({ ready: { eq: true } });
  });

  it("ticking a second output qualifies an existing unqualified clause", () => {
    seedEdge({ ...singlePort, when: { has_design_work: { eq: true } } });
    render(<EdgeDetailPanel />);

    fireEvent.click(within(screen.getByTestId("output-checkbox-spec")).getByRole("checkbox"));

    expect(currentEdge().when).toEqual({ "out.has_design_work": { eq: true } });
  });
});

describe("serialization of a multi-port edge (#843)", () => {
  it("emits `ports:` for several and `port:` for one", () => {
    seedEdge({ ...singlePort, source: { node: "design", ports: ["out", "spec"] } });
    render(<EdgeDetailPanel />);
    let edges = pipelineToYamlObject(useEditStore.getState().openTabs[0].pipeline)
      .edges as Record<string, unknown>[];
    expect(edges[0].source).toEqual({ node: "design", ports: ["out", "spec"] });

    fireEvent.click(within(screen.getByTestId("output-checkbox-spec")).getByRole("checkbox"));

    edges = pipelineToYamlObject(useEditStore.getState().openTabs[0].pipeline)
      .edges as Record<string, unknown>[];
    expect(edges[0].source).toEqual({ node: "design", port: "out" });
  });
});
