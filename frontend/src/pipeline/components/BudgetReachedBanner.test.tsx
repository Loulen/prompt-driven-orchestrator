import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { RunState } from "../../shared/generated/RunState";
import { endRegion } from "../../run/api/commands";
import { useEditStore } from "../store/editStore";
import { BudgetReachedBanner } from "./BudgetReachedBanner";
import { budgetReachedRegion } from "./loopRegionCost";

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
    nodes: {},
    loop_states: {
      "review-loop": { loop_node_id: "review-loop", current_iter: 4, max_iter: 5, break_received: false, done: false },
    },
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
    render(<BudgetReachedBanner run={parkedRun()} regionId="review-loop" />);
    expect(screen.getByText("Budget reached · awaiting you")).toBeInTheDocument();
    expect(screen.getByTestId("run-budget-reason")).toHaveTextContent(
      "Loop review-loop reached its cost cap: ~$5.12 spent of $5 after lap 4/5. Its exit was not followed. Raise the cap and resume, or end the region.",
    );
  });

  it("selects the region from its name and from raise cap, and ends it from end region", () => {
    render(<BudgetReachedBanner run={parkedRun()} regionId="review-loop" />);
    fireEvent.click(screen.getByTestId("run-budget-region"));
    expect(useEditStore.getState().selection).toEqual({ kind: "region", id: null, regionId: "review-loop" });
    useEditStore.setState({ selection: { kind: "none", id: null } });
    fireEvent.click(screen.getByTestId("run-budget-raise-cap"));
    expect(useEditStore.getState().selection).toEqual({ kind: "region", id: null, regionId: "review-loop" });
    fireEvent.click(screen.getByTestId("run-budget-end-region"));
    expect(endRegion).toHaveBeenCalledWith("run-1", "review-loop");
  });
});
