import { describe, it, expect } from "vitest";
import {
  EMPTY_RUN_FILTER,
  activeFilterCount,
  isFilterActive,
  runFilterOptions,
  projectKey,
  runMatchesFilter,
  type RunFilterValue,
} from "./runFilter";
import type { RunListEntry } from "../../shared/generated/RunListEntry";

/* #336 filter model; #783 removed the #725 `showOrchestrated` axis — a child run
   is a tree row under its parent (`lib/runTree.ts`), not a filterable population.
   The filter is a per-run predicate on three axes with AND semantics. */

const run = (over: Partial<RunListEntry>): RunListEntry => ({
  stalled: false,
  effective_repo: "",
  run_id: "r",
  pipeline_name: "p",
  status: "running",
  started_at: null,
  ...over,
});

const root = run({ run_id: "root" });
const child = run({ run_id: "child", parent_run_id: "root" });

describe("runFilter — three axes, filiation-blind (#783)", () => {
  it("the empty filter matches roots and children alike", () => {
    expect(runMatchesFilter(child, EMPTY_RUN_FILTER)).toBe(true);
    expect(runMatchesFilter(root, EMPTY_RUN_FILTER)).toBe(true);
    expect("showOrchestrated" in EMPTY_RUN_FILTER).toBe(false);
  });

  it("composes the axes with AND semantics", () => {
    const alpha = run({ run_id: "a", effective_repo: "/repos/alpha", pipeline_name: "impl" });
    const f: RunFilterValue = {
      ...EMPTY_RUN_FILTER,
      repo: "/repos/alpha",
      pipeline: "impl",
    };
    expect(runMatchesFilter(alpha, f)).toBe(true);
    expect(runMatchesFilter(alpha, { ...f, pipeline: "other" })).toBe(false);
    expect(runMatchesFilter(alpha, { ...f, repo: "/repos/beta" })).toBe(false);
  });

  it("composes the Date axis with the others", () => {
    const now = new Date(2026, 9, 14, 15);
    const startedYesterday = run({
      run_id: "y",
      pipeline_name: "impl",
      started_at: new Date(2026, 9, 13, 10).toISOString(),
      activity: [],
    });
    const yesterday = { ...EMPTY_RUN_FILTER, date: { mode: "started" as const, range: { preset: "yesterday" as const } } };
    expect(runMatchesFilter(startedYesterday, yesterday, undefined, now)).toBe(true);
    expect(runMatchesFilter(startedYesterday, { ...yesterday, pipeline: "other" }, undefined, now)).toBe(false);
    const today = { ...yesterday, date: { mode: "started" as const, range: { preset: "today" as const } } };
    expect(runMatchesFilter(startedYesterday, today, undefined, now)).toBe(false);
  });

  it("isFilterActive reports exactly the four axes", () => {
    expect(isFilterActive(EMPTY_RUN_FILTER)).toBe(false);
    expect(isFilterActive({ ...EMPTY_RUN_FILTER, repo: "/repos/x" })).toBe(true);
    expect(isFilterActive({ ...EMPTY_RUN_FILTER, pipeline: "p" })).toBe(true);
    expect(isFilterActive({ ...EMPTY_RUN_FILTER, trigger: "t" })).toBe(true);
    expect(isFilterActive({ ...EMPTY_RUN_FILTER, date: { mode: "started", range: null } })).toBe(false);
    expect(isFilterActive({ ...EMPTY_RUN_FILTER, date: { mode: "active", range: { preset: "today" } } })).toBe(true);
  });
});

describe("runFilter — the Project axis follows the recorded Projet (ADR-0087)", () => {
  const projects = new Map([
    ["p1", { id: "p1", name: "Checkout" }],
    ["p2", { id: "p2", name: "Mobile" }],
  ]);
  const projectOf = (id: string | null | undefined) => (id ? projects.get(id) ?? null : null);
  const inP1 = run({ run_id: "a", effective_repo: "/code/shop-api", project_id: "p1" });
  const inP2 = run({ run_id: "b", effective_repo: "/code/shop-api", project_id: "p2" });
  const inNone = run({ run_id: "c", effective_repo: "/code/shop-api" });
  const inGone = run({ run_id: "d", effective_repo: "/code/shop-api", project_id: "deleted" });

  it("buckets a Run by its Projet, else by its repo", () => {
    expect(projectKey(inP1, projectOf)).toBe("project:p1");
    expect(projectKey(inNone, projectOf)).toBe("/code/shop-api");
    expect(projectKey(inGone, projectOf)).toBe("/code/shop-api");
  });

  it("never mixes the Projets of a repo shared between them", () => {
    const f = { ...EMPTY_RUN_FILTER, repo: "project:p1" };
    expect(runMatchesFilter(inP1, f, projectOf)).toBe(true);
    expect(runMatchesFilter(inP2, f, projectOf)).toBe(false);
    expect(runMatchesFilter(inNone, f, projectOf)).toBe(false);
    const byRepo = { ...EMPTY_RUN_FILTER, repo: "/code/shop-api" };
    expect(runMatchesFilter(inNone, byRepo, projectOf)).toBe(true);
    expect(runMatchesFilter(inP2, byRepo, projectOf)).toBe(false);
  });
});

describe("runFilter — the menu's options and its count (funnel)", () => {
  const projectOf = (id: string | null | undefined) => (id === "p1" ? { id: "p1", name: "Checkout" } : null);
  const runs = [
    run({ run_id: "a", effective_repo: "/code/zebra", pipeline_name: "deploy", triggered_by: "t1" }),
    run({ run_id: "b", effective_repo: "/code/shop", pipeline_name: "audit", project_id: "p1" }),
    run({ run_id: "c", effective_repo: "/code/alpha", pipeline_name: "", triggered_by: "gone" }),
    run({ run_id: "d", effective_repo: "/code/alpha", pipeline_name: "audit" }),
  ];

  it("lists the Projets by name first, then the repos by basename; each value once", () => {
    const { project } = runFilterOptions(runs, [], projectOf);
    expect(project).toEqual([
      { value: "project:p1", label: "Checkout" },
      { value: "/code/alpha", label: "alpha" },
      { value: "/code/zebra", label: "zebra" },
    ]);
  });

  it("names a nameless pipeline, the manual launches and a deleted trigger by its id", () => {
    const { pipeline, trigger } = runFilterOptions(runs, [{ id: "t1", name: "Nightly" }], projectOf);
    expect(pipeline.map((o) => o.label)).toEqual(["(unnamed)", "audit", "deploy"]);
    expect(trigger).toEqual([
      { value: "__manual__", label: "Manual" },
      { value: "gone", label: "gone" },
      { value: "t1", label: "Nightly" },
    ]);
  });

  it("counts each axis set, the date range and each extension attribute", () => {
    expect(activeFilterCount(EMPTY_RUN_FILTER)).toBe(0);
    const f: RunFilterValue = {
      ...EMPTY_RUN_FILTER,
      repo: "project:p1",
      pipeline: "audit",
      date: { mode: "started", range: { preset: "7d" } },
    };
    expect(activeFilterCount(f)).toBe(3);
    expect(activeFilterCount(f, { "x/a": "1", "x/b": "2" }, { "x/c": "3" })).toBe(6);
    // The mode alone narrows nothing.
    expect(activeFilterCount({ ...EMPTY_RUN_FILTER, date: { mode: "started", range: null } })).toBe(0);
  });
});
