import { act, renderHook } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { RunListEntry } from "../../../shared/generated/RunListEntry";
import type { Project } from "../../../workspace/model/project";
import { useRunTreeView } from "./useRunTreeView";

function run(
  runId: string,
  attributes?: Record<string, string>,
  repo = "/repo",
  projectId?: string,
): RunListEntry {
  return {
    run_id: runId,
    pipeline_name: "solo",
    status: "running",
    stalled: false,
    started_at: "2026-09-28T00:00:00Z",
    effective_repo: repo,
    project_id: projectId,
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

  it("keeps only the Runs of a Projet carrying the attributes of the projects/filter (#31)", () => {
    // ADR-0087: the Projet recorded on each Run, not its repository's.
    const runs = [
      run("core", undefined, "/repos/shared", "p1"),
      run("ext", undefined, "/repos/shared", "p2"),
      run("loose", undefined, "/repos/shared"),
    ];
    const projects = [
      { id: "p1", name: "Core", members: ["/repos/shared"], attributes: { "test/team": "core" } },
      { id: "p2", name: "Ext", members: ["/repos/shared"] },
    ] as unknown as Project[];
    const { result } = renderHook(() => useRunTreeView(runs, null, projects));
    expect(result.current.filteredRuns).toHaveLength(3);

    act(() => result.current.setProjectFilter({ "test/team": "core" }));
    expect(result.current.filteredRuns.map((r) => r.run_id)).toEqual(["core"]);
    expect(result.current.filterActive).toBe(true);

    act(() => result.current.setProjectFilter({}));
    expect(result.current.filteredRuns).toHaveLength(3);
  });

  it("applies the Date filter to the active and the archived Runs alike", () => {
    const long = { ...run("long"), started_at: "2020-01-01T12:00:00Z" };
    const oldArchived = { ...run("old"), status: "archived" as const, started_at: "2020-01-01T12:00:00Z" };
    const fresh = { ...run("fresh"), started_at: new Date().toISOString() };
    const freshArchived = { ...fresh, run_id: "fresh-archived", status: "archived" as const };
    const { result } = renderHook(() => useRunTreeView([long, oldArchived, fresh, freshArchived], null));
    expect(result.current.archivedCount).toBe(2);

    act(() =>
      result.current.setRunFilter({
        ...result.current.runFilter,
        date: { mode: "started", range: { preset: "today" } },
      }),
    );
    expect(result.current.filteredRuns.map((r) => r.run_id)).toEqual(["fresh", "fresh-archived"]);
    expect(result.current.archivedCount).toBe(1);
    expect(result.current.filterActive).toBe(true);
  });
});
