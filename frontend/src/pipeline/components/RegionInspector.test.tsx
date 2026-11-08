import { act, render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, beforeEach, vi } from "vitest";
import RegionInspector from "./RegionInspector";
import { useEditStore } from "../store/editStore";
import type { LoopRegion } from "../../shared/generated/LoopRegion";
import type { EditorNodeDef, EditorPipelineDef } from "../model/pipeline";
import type { RunState } from "../../shared/generated/RunState";

// The catalogue says which harness PDO can read the cost of: `opencode` cannot.
vi.mock("../../harness/api/catalog", () => ({
  fetchHarnessCatalog: vi.fn().mockResolvedValue({
    builtin: [
      { name: "claude", installed: true, models: [], modelContexts: {}, efforts: [], hasEffort: true, version: null, canCost: true },
      { name: "opencode", installed: true, models: [], modelContexts: {}, efforts: [], hasEffort: false, version: null, canCost: false },
    ],
    descriptors: [],
  }),
}));

function makeNode(id: string, name: string): EditorNodeDef {
  return {
    id,
    name,
    type: "agent",
    inputs: [],
    outputs: [{ name: "out" }],
    interactive: false,
  };
}

function makePipeline(loops: LoopRegion[]): EditorPipelineDef {
  return {
    name: "test-pipeline",
    variables: {},
    nodes: [makeNode("impl", "implementer"), makeNode("rev", "reviewer")],
    edges: [],
    loops,
  };
}

function selectRegion(loops: LoopRegion[], regionId: string) {
  useEditStore.setState({
    openTabs: [
      {
        id: "tab1",
        scope: "repo",
        pipeline: makePipeline(loops),
        prompts: {},
        diagnostics: [],
        dirty: false,
        externalDirty: false,
      },
    ],
    activeTabId: "tab1",
    selection: { kind: "region", id: null, regionId },
  });
}

const reviewLoop: LoopRegion = {
  id: "review_loop",
  kind: "bounded",
  members: ["impl", "rev"],
  max_iter: 3,
};

describe("RegionInspector (#150)", () => {
  beforeEach(() => {
    useEditStore.setState({
      openTabs: [],
      activeTabId: null,
      selection: { kind: "none", id: null },
    });
  });

  it("renders nothing when the selection is not a region", () => {
    useEditStore.setState({
      openTabs: [
        {
          id: "tab1",
          scope: "repo",
          pipeline: makePipeline([reviewLoop]),
          prompts: {},
          diagnostics: [],
          dirty: false,
          externalDirty: false,
        },
      ],
      activeTabId: "tab1",
      selection: { kind: "none", id: null },
    });
    const { container } = render(<RegionInspector />);
    expect(container.firstChild).toBeNull();
  });

  it("shows the selected region's id and members", () => {
    selectRegion([reviewLoop], "review_loop");
    render(<RegionInspector />);
    expect(screen.getByText("review_loop")).toBeInTheDocument();
    // Member node names are listed.
    expect(screen.getByText(/implementer/)).toBeInTheDocument();
    expect(screen.getByText(/reviewer/)).toBeInTheDocument();
  });

  it("exposes max_iter in an editable field seeded with the current bound", () => {
    selectRegion([reviewLoop], "review_loop");
    render(<RegionInspector />);
    const input = screen.getByTestId("region-max-iter");
    expect(input).toHaveValue(3);
  });

  it("commits a new max_iter to the region via updateRegion", () => {
    selectRegion([reviewLoop], "review_loop");
    render(<RegionInspector />);
    const input = screen.getByTestId("region-max-iter");
    fireEvent.change(input, { target: { value: "8" } });
    const tab = useEditStore.getState().openTabs[0];
    expect(tab.pipeline.loops!.find((r) => r.id === "review_loop")!.max_iter).toBe(8);
  });
});

function regionOf(): LoopRegion | undefined {
  return useEditStore.getState().openTabs[0].pipeline.loops!.find((r) => r.id === "review_loop");
}

function runWith(over: Partial<RunState> = {}): RunState {
  return {
    run_id: "run-1",
    pipeline_name: "p",
    status: "running",
    input: null,
    started_at: null,
    completed_at: null,
    loop_states: {},
    foreach_states: {},
    collection_states: {},
    switch_states: {},
    sandbox: "off",
    sessions_spawned: 0,
    edges: [],
    node_defs: [],
    nodes: {
      impl: { node_id: "impl", status: "completed", iter: 4, started_at: null, completed_at: null, failure_reason: null, iterations: [], frontmatter_retries: 0 },
      rev: { node_id: "rev", status: "completed", iter: 4, started_at: null, completed_at: null, failure_reason: null, iterations: [], frontmatter_retries: 0 },
    },
    ...over,
  } as unknown as RunState;
}

describe("RegionInspector — Plafond de coût", () => {
  beforeEach(() => {
    useEditStore.setState({ openTabs: [], activeTabId: null, selection: { kind: "none", id: null } });
  });

  it("sets a positive cap, and removes it with ✕", () => {
    selectRegion([reviewLoop], "review_loop");
    render(<RegionInspector />);
    const input = screen.getByTestId("region-max-cost");
    expect(input).toHaveAttribute("placeholder", "no cap");
    fireEvent.change(input, { target: { value: "5" } });
    expect(regionOf()!.max_cost_usd).toBe(5);
    fireEvent.click(screen.getByTestId("region-max-cost-clear"));
    expect(regionOf()!.max_cost_usd).toBeNull();
    expect(input).toHaveValue("");
  });

  it("refuses 0 and a non-number with a reason, and never writes them", () => {
    selectRegion([{ ...reviewLoop, max_cost_usd: 2 }], "review_loop");
    render(<RegionInspector />);
    const input = screen.getByTestId("region-max-cost");
    fireEvent.change(input, { target: { value: "0" } });
    expect(screen.getByRole("alert")).toHaveTextContent("Must be above 0");
    expect(regionOf()!.max_cost_usd).toBe(2);
    fireEvent.change(input, { target: { value: "five" } });
    expect(screen.getByRole("alert")).toHaveTextContent("A number or a $var");
    expect(regionOf()!.max_cost_usd).toBe(2);
    fireEvent.change(input, { target: { value: "0.5" } });
    expect(screen.queryByRole("alert")).toBeNull();
    expect(regionOf()!.max_cost_usd).toBe(0.5);
  });

  it("accepts a declared $var, and refuses an unknown one", () => {
    selectRegion([reviewLoop], "review_loop");
    const tab = useEditStore.getState().openTabs[0];
    useEditStore.setState({
      openTabs: [{ ...tab, pipeline: { ...tab.pipeline, variables: { budget: { type: "float", default: 2 } } } }],
    });
    render(<RegionInspector />);
    fireEvent.change(screen.getByTestId("region-max-cost"), { target: { value: "$nope" } });
    expect(screen.getByRole("alert")).toHaveTextContent("No such variable");
    expect(regionOf()!.max_cost_usd).toBeUndefined();
    fireEvent.change(screen.getByTestId("region-max-cost"), { target: { value: "$budget" } });
    expect(regionOf()!.max_cost_usd).toBe("$budget");
  });

  it("shows the value the Run was launched with for a $var cap", () => {
    selectRegion([{ ...reviewLoop, max_cost_usd: "$budget" }], "review_loop");
    const tab = useEditStore.getState().openTabs[0];
    useEditStore.setState({
      openTabs: [{ ...tab, pipeline: { ...tab.pipeline, variables: { budget: { type: "float", default: 0.5 } } } }],
    });
    render(<RegionInspector runState={runWith({ variables: { budget: 10 } })} />);
    expect(screen.getByText(/≤ \$10/)).toBeInTheDocument();
  });

  it("forgets a refused draft when another region is selected", () => {
    const other: LoopRegion = { id: "other_loop", kind: "bounded", members: ["impl"], max_iter: 2, max_cost_usd: 3 };
    selectRegion([reviewLoop, other], "review_loop");
    render(<RegionInspector />);
    fireEvent.change(screen.getByTestId("region-max-cost"), { target: { value: "0" } });
    expect(screen.getByRole("alert")).toBeInTheDocument();
    act(() => useEditStore.setState({ selection: { kind: "region", id: null, regionId: "other_loop" } }));
    expect(screen.getByTestId("region-max-cost")).toHaveValue("3");
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("has no cost cap on a collection, and says why", () => {
    selectRegion([{ id: "review_loop", kind: "collection", members: ["impl"], over: "items" }], "review_loop");
    render(<RegionInspector />);
    expect(screen.queryByTestId("region-max-cost")).toBeNull();
    expect(screen.getByTestId("region-cap-collection")).toHaveTextContent("cost caps apply to bounded loops only");
  });

  it("warns that a member with no cost source may overshoot the cap", async () => {
    selectRegion([{ ...reviewLoop, max_cost_usd: 5 }], "review_loop");
    const tab = useEditStore.getState().openTabs[0];
    tab.pipeline.nodes[1] = { ...tab.pipeline.nodes[1], pin_harness: "opencode" };
    useEditStore.setState({ openTabs: [{ ...tab }] });
    render(<RegionInspector />);
    await waitFor(() => expect(screen.getByTestId("region-cap-overshoot")).toHaveTextContent("Cap may be overshot"));
    expect(screen.getByTestId("region-cap-overshoot")).toHaveTextContent("reviewer");
    expect(screen.getByTestId("region-member-no-cost")).toHaveTextContent("no cost source");
  });

  it("shows the loop cost against its cap on a Run, amber from 80 %", () => {
    selectRegion([{ ...reviewLoop, max_iter: 5, max_cost_usd: 5 }], "review_loop");
    const run = runWith({
      loop_costs: {
        review_loop: {
          usd: 4.2, estimated: true, partial: false, executions: 8, readable_executions: 8, cap_usd: 5,
          members: [
            { node_id: "impl", usd: 3.1, estimated: true, partial: false, executions: 4 },
            { node_id: "rev", usd: 1.1, estimated: true, partial: false, executions: 4 },
          ],
        },
      },
    });
    render(<RegionInspector runState={run} locked />);
    const section = screen.getByTestId("region-loop-cost");
    expect(section).toHaveTextContent("~$4.20 / $5");
    expect(section).toHaveTextContent("84 % of cap · laps 1–4");
    expect(section.querySelector("[data-warn]")).not.toBeNull();
    expect(screen.getAllByTestId("region-member-cost").map((e) => e.textContent)).toEqual(["~$3.10", "~$1.10"]);
    expect(screen.getByTestId("region-followed-note")).toHaveTextContent("Edit for this run");
  });

  it("shows no Loop cost section off a Run", () => {
    selectRegion([{ ...reviewLoop, max_cost_usd: 5 }], "review_loop");
    render(<RegionInspector />);
    expect(screen.queryByTestId("region-loop-cost")).toBeNull();
  });
});
