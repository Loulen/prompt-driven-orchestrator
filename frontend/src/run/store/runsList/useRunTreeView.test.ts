import { act, renderHook } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { RunListEntry } from "../../../shared/generated/RunListEntry";
import { useRunTreeView } from "./useRunTreeView";

function run(runId: string, attributes?: Record<string, string>): RunListEntry {
  return {
    run_id: runId,
    pipeline_name: "solo",
    status: "running",
    stalled: false,
    started_at: "2026-09-28T00:00:00Z",
    effective_repo: "/repo",
    attributes,
  };
}

describe("useRunTreeView", () => {
  it("keeps only the Runs carrying the attributes an extension's filter asks for", () => {
    const runs = [run("hers", { "test/started_by": "alice" }), run("mine")];
    const { result } = renderHook(() => useRunTreeView(runs, null));
    expect(result.current.filteredRuns.map((r) => r.run_id)).toEqual(["hers", "mine"]);
    expect(result.current.filterActive).toBe(false);

    act(() => result.current.setAttributeFilter({ "test/started_by": "alice" }));
    expect(result.current.filteredRuns.map((r) => r.run_id)).toEqual(["hers"]);
    expect(result.current.filterActive).toBe(true);

    act(() => result.current.setAttributeFilter({}));
    expect(result.current.filteredRuns).toHaveLength(2);
  });
});
