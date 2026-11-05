import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, beforeEach, vi } from "vitest";
import RegionInspector from "./RegionInspector";
import { useEditStore } from "../stores/editStore";
import type { PipelineDef, NodeDef, LoopRegion, RunState } from "../types";
import { endRegion } from "../api";

// The harness catalogue is served by `GET /settings`; the inspector falls back
// to the embedded floor (claude: costable, opencode: no cost source).
vi.mock("../api", () => ({
  fetchSettings: vi.fn().mockRejectedValue(new Error("offline")),
  endRegion: vi.fn().mockResolvedValue(undefined),
}));

function makeNode(id: string, name: string): NodeDef {
  return {
    id,
    name,
    type: "agent",
    inputs: [],
    outputs: [{ name: "out" }],
    interactive: false,
  };
}

function makePipeline(loops: LoopRegion[]): PipelineDef {
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

describe("RegionInspector — Plafond de coût (#1016)", () => {
  beforeEach(() => {
    useEditStore.setState({
      openTabs: [],
      activeTabId: null,
      selection: { kind: "none", id: null },
    });
  });

  function selectWith(loops: LoopRegion[], nodes?: NodeDef[]) {
    selectRegion(loops, loops[0].id);
    if (nodes) {
      useEditStore.setState((s) => ({
        openTabs: s.openTabs.map((t) => ({ ...t, pipeline: { ...t.pipeline, nodes } })),
      }));
    }
  }

  const region = () => useEditStore.getState().openTabs[0].pipeline.loops![0];

  it("exposes max_cost_usd under max_iter, empty = no cap", () => {
    selectWith([reviewLoop]);
    render(<RegionInspector />);
    const input = screen.getByTestId("region-max-cost");
    expect(input).toHaveValue(null);
    expect(input).toHaveAttribute("placeholder", "no cap");
    expect(screen.queryByTestId("region-max-cost-clear")).toBeNull();
  });

  it("commits a cap, and ✕ removes it", () => {
    selectWith([reviewLoop]);
    render(<RegionInspector />);
    fireEvent.change(screen.getByTestId("region-max-cost"), { target: { value: "2.5" } });
    expect(region().max_cost_usd).toBe(2.5);
    fireEvent.click(screen.getByTestId("region-max-cost-clear"));
    expect(region().max_cost_usd).toBeNull();
  });

  it("edits a `$var` cap as text", () => {
    selectWith([{ ...reviewLoop, max_cost_usd: "$budget" }]);
    render(<RegionInspector />);
    const input = screen.getByTestId("region-max-cost");
    expect(input).toHaveAttribute("type", "text");
    expect(input).toHaveValue("$budget");
  });

  it("is absent on a collection region, which says caps are bounded-only", () => {
    selectWith([{ id: "fan", kind: "collection", members: ["impl"], over: "items" }]);
    render(<RegionInspector />);
    expect(screen.queryByTestId("region-max-cost")).toBeNull();
    expect(screen.getByTestId("region-no-cap-note")).toHaveTextContent(
      "Cost caps apply to bounded loops only.",
    );
  });

  it("warns when a capped loop has a member with no cost source", () => {
    const nodes: NodeDef[] = [
      { id: "impl", name: "implementer", type: "agent", inputs: [], outputs: [], interactive: false },
      {
        id: "rev",
        name: "reviewer",
        type: "agent",
        inputs: [],
        outputs: [],
        interactive: false,
        pin_harness: "opencode",
      },
    ];
    selectWith([{ ...reviewLoop, max_cost_usd: 2 }], nodes);
    render(<RegionInspector />);
    const warning = screen.getByTestId("region-cost-warning");
    expect(warning).toHaveTextContent("Cap may be overshot");
    expect(warning).toHaveTextContent("reviewer runs on opencode, which has no cost source");
    expect(screen.getByTestId("region-member-rev")).toHaveTextContent("no cost source");
    expect(screen.getByTestId("region-member-impl")).not.toHaveTextContent("no cost source");
  });

  it("does not warn when every member is costable", () => {
    selectWith([{ ...reviewLoop, max_cost_usd: 2 }]);
    render(<RegionInspector />);
    expect(screen.queryByTestId("region-cost-warning")).toBeNull();
  });

  it("focuses the cap when opened by `raise cap`", () => {
    selectWith([{ ...reviewLoop, max_cost_usd: 5 }]);
    useEditStore.setState({
      selection: { kind: "region", id: null, regionId: "review_loop", focusField: "max_cost_usd" },
    });
    render(<RegionInspector />);
    expect(screen.getByTestId("region-max-cost")).toHaveFocus();
  });

  function liveRun(usd: number, partial = false): RunState {
    const member = (id: string) => ({
      node_id: id,
      status: "completed" as const,
      iter: 3,
      started_at: null,
      completed_at: null,
      failure_reason: null,
      iterations: [],
      harness: "claude",
    });
    return {
      run_id: "a1b2c3d4e5",
      pipeline_name: "p",
      status: "running",
      input: null,
      started_at: null,
      completed_at: null,
      nodes: { impl: member("impl"), rev: member("rev") },
      edges: [],
      node_defs: [],
      start_node: null,
      end_node: null,
      merge_resolver: null,
      loop_costs: {
        review_loop: {
          usd,
          estimated: true,
          partial,
          executions: 6,
          readable_executions: 6,
          members: [
            { node_id: "impl", usd: 0.82, estimated: true, partial: false, executions: 3 },
            { node_id: "rev", usd: 0.38, estimated: true, partial: false, executions: 3 },
          ],
          cap_usd: 5,
        },
      },
    };
  }

  it("shows the loop cost against the cap on a live run, and each member's share", () => {
    selectWith([{ ...reviewLoop, max_iter: 5, max_cost_usd: 5 }]);
    render(<RegionInspector run={liveRun(1.2)} />);
    expect(screen.getByTestId("region-loop-cost-amount")).toHaveTextContent("~$1.20 / $5");
    expect(screen.getByTestId("region-loop-cost-window")).toHaveTextContent("24 % of cap · laps 1–3");
    expect(screen.getByTestId("region-loop-cost-meter").firstElementChild).toHaveAttribute(
      "data-tone",
      "normal",
    );
    expect(screen.getByTestId("region-member-impl")).toHaveTextContent("~$0.8200");
  });

  it("turns the meter amber from 80 % of the cap", () => {
    selectWith([{ ...reviewLoop, max_iter: 5, max_cost_usd: 5 }]);
    render(<RegionInspector run={liveRun(4.31)} />);
    expect(screen.getByTestId("region-loop-cost-meter").firstElementChild).toHaveAttribute(
      "data-tone",
      "warn",
    );
  });

  it("warns when the current loop cost is partial", () => {
    selectWith([{ ...reviewLoop, max_iter: 5, max_cost_usd: 5 }]);
    render(<RegionInspector run={liveRun(1.2, true)} />);
    expect(screen.getByTestId("region-cost-warning")).toHaveTextContent("partial");
  });

  it("says where editing lives while following a run", () => {
    selectWith([{ ...reviewLoop, max_cost_usd: 5 }]);
    render(<RegionInspector run={liveRun(1.2)} locked />);
    expect(screen.getByTestId("region-locked-note")).toHaveTextContent("Edit for this run");
  });

  it("shows no loop cost section on a template", () => {
    selectWith([{ ...reviewLoop, max_cost_usd: 5 }]);
    render(<RegionInspector />);
    expect(screen.queryByTestId("region-loop-cost")).toBeNull();
  });

  // ---- #1017: « Edit for this run » — hot editing the cap ----

  const budgetPark = (run: RunState): RunState => ({
    ...run,
    status: "awaiting_user",
    awaiting_reason_code: "region_budget_exhausted",
    awaiting_reason: "budget reached: …",
    awaiting_region_id: "review_loop",
  });

  it("in « Edit for this run », says the budget was reached and that saving resumes", () => {
    selectWith([{ ...reviewLoop, max_iter: 5, max_cost_usd: 5 }]);
    render(<RegionInspector run={budgetPark(liveRun(5.12))} />);
    expect(screen.getByTestId("region-budget-reached")).toHaveTextContent(
      "Budget reached · Run awaiting you",
    );
    expect(screen.getByTestId("region-loop-cost")).toBeInTheDocument();
    expect(screen.getByTestId("region-run-edit-note")).toHaveTextContent(
      "Saving resumes the Run at the next lap",
    );
    expect(screen.getByTestId("region-inspector")).toHaveAttribute("data-run-editing", "true");
    // The field is editable and the edit lands in the run tab.
    fireEvent.change(screen.getByTestId("region-max-cost"), { target: { value: "8" } });
    expect(region().max_cost_usd).toBe(8);
  });

  it("offers `end region` once the budget is reached", () => {
    vi.mocked(endRegion).mockClear();
    selectWith([{ ...reviewLoop, max_iter: 5, max_cost_usd: 5 }]);
    render(<RegionInspector run={budgetPark(liveRun(5.12))} />);
    fireEvent.click(screen.getByTestId("region-end-region"));
    expect(endRegion).toHaveBeenCalledWith("a1b2c3d4e5", "review_loop");
  });

  it("accepts a cap below the spend with an amber note (Q8)", () => {
    selectWith([{ ...reviewLoop, max_iter: 5, max_cost_usd: 1 }]);
    render(<RegionInspector run={liveRun(1.2)} />);
    expect(screen.getByTestId("region-cost-below-spent")).toHaveTextContent(
      "Below the ~$1.20 already spent: the loop will stop at the next re-entry.",
    );
    expect(screen.queryByTestId("region-budget-reached")).toBeNull();
  });

  it("does not show the below-spent note for a cap above the spend", () => {
    selectWith([{ ...reviewLoop, max_iter: 5, max_cost_usd: 5 }]);
    render(<RegionInspector run={liveRun(1.2)} />);
    expect(screen.queryByTestId("region-cost-below-spent")).toBeNull();
  });

  it("while following, the budget box points to « Edit for this run » and offers no end button", () => {
    selectWith([{ ...reviewLoop, max_iter: 5, max_cost_usd: 5 }]);
    render(<RegionInspector run={budgetPark(liveRun(5.12))} locked />);
    expect(screen.getByTestId("region-budget-reached")).toHaveTextContent("Edit for this run");
    expect(screen.queryByTestId("region-end-region")).toBeNull();
  });

  it("focuses the cap once « Edit for this run » unlocks it (`raise cap`)", () => {
    selectWith([{ ...reviewLoop, max_iter: 5, max_cost_usd: 5 }]);
    useEditStore.setState({
      selection: { kind: "region", id: null, regionId: "review_loop", focusField: "max_cost_usd" },
    });
    const { rerender } = render(
      <fieldset disabled>
        <RegionInspector run={budgetPark(liveRun(5.12))} locked />
      </fieldset>,
    );
    expect(screen.getByTestId("region-max-cost")).not.toHaveFocus();
    rerender(
      <fieldset disabled={false}>
        <RegionInspector run={budgetPark(liveRun(5.12))} locked={false} />
      </fieldset>,
    );
    expect(screen.getByTestId("region-max-cost")).toHaveFocus();
  });
});
