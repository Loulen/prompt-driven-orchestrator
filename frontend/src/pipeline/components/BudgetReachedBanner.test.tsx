import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { RunState } from "../../shared/generated/RunState";
import { endRegion } from "../../run/api/commands";
import { useEditStore } from "../store/editStore";
import { BudgetReachedBanner } from "./BudgetReachedBanner";
import { budgetReachedRegion } from "./loopRegionCost";
import type { EditorPipelineDef } from "../model/pipeline";

vi.mock("../../run/api/commands", () => ({
  endRegion: vi.fn().mockResolvedValue(undefined),
}));

function parkedRun(over: Partial<RunState> = {}): RunState {
  return {
    run_id: "run-1",
    pipeline_name: "p",
    status: "awaiting_user",
    awaiting_reason_code: "region_budget_exhausted",
    awaiting_reason: "budget reached: bounded region 'review-loop' reached its cost cap",
    awaiting_region_id: "review-loop",
    input: null,
    started_at: null,
    completed_at: null,
    // The daemon stamps every member with the region's lap; a region has no `loop_states` entry.
    nodes: { implement: { iter: 4 }, review: { iter: 3 } },
    loop_states: {},
    foreach_states: {},
    collection_states: {},
    switch_states: {},
    sandbox: "off",
    sessions_spawned: 0,
    loop_costs: {
      "review-loop": {
        usd: 5.12, estimated: true, partial: false, executions: 8, readable_executions: 8, members: [], cap_usd: 5,
      },
    },
    ...over,
  } as unknown as RunState;
}

const PIPELINE = {
  name: "p",
  version: null,
  variables: {},
  nodes: [],
  edges: [],
  loops: [{ id: "review-loop", kind: "bounded", members: ["implement", "review"], max_iter: 5 }],
} as unknown as EditorPipelineDef;

describe("BudgetReachedBanner", () => {
  beforeEach(() => {
    vi.mocked(endRegion).mockClear();
    useEditStore.setState({ selection: { kind: "none", id: null } });
  });

  it("names the region a budget park is about, and nothing else", () => {
    expect(budgetReachedRegion(parkedRun())).toBe("review-loop");
    expect(budgetReachedRegion(parkedRun({ awaiting_reason_code: "region_exhausted" }))).toBeNull();
    expect(budgetReachedRegion(parkedRun({ status: "running" }))).toBeNull();
    expect(budgetReachedRegion(null)).toBeNull();
  });

  it("explains the stop: the loop, what it spent of its cap, after which lap", () => {
    render(<BudgetReachedBanner run={parkedRun()} regionId="review-loop" pipeline={PIPELINE} />);
    expect(screen.getByText("Budget reached · awaiting you")).toBeInTheDocument();
    expect(screen.getByTestId("run-budget-reason")).toHaveTextContent(
      "Loop review-loop reached its cost cap: ~$5.12 spent of $5 after lap 4/5. Its exit was not followed. Raise the cap and resume, or end the region.",
    );
  });

  it("leaves the lap out when the Run's pipeline is not at hand", () => {
    render(<BudgetReachedBanner run={parkedRun()} regionId="review-loop" pipeline={null} />);
    expect(screen.getByTestId("run-budget-reason")).toHaveTextContent(
      "Loop review-loop reached its cost cap: ~$5.12 spent of $5. Its exit was not followed.",
    );
  });

  it("selects the region from its name, giving the panel way to its inspector", () => {
    const onShowRegion = vi.fn();
    render(
      <BudgetReachedBanner run={parkedRun()} regionId="review-loop" pipeline={PIPELINE} onShowRegion={onShowRegion} />,
    );
    fireEvent.click(screen.getByTestId("run-budget-region"));
    expect(useEditStore.getState().selection).toEqual({ kind: "region", id: null, regionId: "review-loop" });
    expect(onShowRegion).toHaveBeenCalledTimes(1);
  });

  it("raise cap edits the Run for itself, on the region's max_cost_usd, from the cap the Run applies", () => {
    const onShowRegion = vi.fn();
    useEditStore.setState({
      openTabs: [
        {
          id: "__run__run-1",
          scope: "run",
          pipeline: structuredClone(PIPELINE),
          prompts: {},
          diagnostics: [],
          dirty: false,
          externalDirty: false,
          runId: "run-1",
          libraryId: null,
          libraryScope: null,
        },
      ],
      activeTabId: "__run__run-1",
      selection: { kind: "none", id: null },
    });
    render(
      <BudgetReachedBanner
        run={parkedRun({ region_max_cost_overrides: { "review-loop": 4 } })}
        regionId="review-loop"
        pipeline={PIPELINE}
        onShowRegion={onShowRegion}
      />,
    );
    fireEvent.click(screen.getByTestId("run-budget-raise-cap"));
    expect(useEditStore.getState().selection).toEqual({
      kind: "region",
      id: null,
      regionId: "review-loop",
      focusField: "max_cost_usd",
    });
    const tab = useEditStore.getState().openTabs[0];
    expect(tab.runEditing).toBe(true);
    expect(tab.pipeline.loops?.[0].max_cost_usd).toBe(4);
    expect(tab.dirty).toBe(false);
    expect(onShowRegion).toHaveBeenCalledTimes(1);
  });

  it("raise cap brings the Run's tab to the front when another tab is", () => {
    const runTab = {
      id: "__run__run-1",
      scope: "run" as const,
      pipeline: structuredClone(PIPELINE),
      prompts: {},
      diagnostics: [],
      dirty: false,
      externalDirty: false,
      runId: "run-1",
      libraryId: null,
      libraryScope: null,
    };
    useEditStore.setState({
      openTabs: [runTab, { ...runTab, id: "other", scope: "repo" as const, runId: undefined }],
      activeTabId: "other",
      selection: { kind: "none", id: null },
    });
    render(<BudgetReachedBanner run={parkedRun()} regionId="review-loop" pipeline={PIPELINE} />);
    fireEvent.click(screen.getByTestId("run-budget-raise-cap"));
    const state = useEditStore.getState();
    expect(state.activeTabId).toBe("__run__run-1");
    expect(state.openTabs[0].runEditing).toBe(true);
    expect(state.selection).toMatchObject({ regionId: "review-loop", focusField: "max_cost_usd" });
  });

  it("ends the region from end region", () => {
    render(<BudgetReachedBanner run={parkedRun()} regionId="review-loop" pipeline={PIPELINE} />);
    fireEvent.click(screen.getByTestId("run-budget-end-region"));
    expect(endRegion).toHaveBeenCalledWith("run-1", "review-loop");
  });
});
