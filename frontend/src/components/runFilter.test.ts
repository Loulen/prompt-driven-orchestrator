import { describe, it, expect } from "vitest";
import {
  EMPTY_RUN_FILTER,
  isFilterActive,
  runMatchesFilter,
  type RunFilterValue,
} from "./runFilter";
import type { RunListEntry } from "../types";

/* #336 filter model; #783 removed the #725 `showOrchestrated` axis — a child run
   is a tree row under its parent (`lib/runTree.ts`), not a filterable population.
   The filter is a per-run predicate on three axes with AND semantics. */

const run = (over: Partial<RunListEntry>): RunListEntry => ({
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
    const f: RunFilterValue = { repo: "/repos/alpha", pipeline: "impl", trigger: null };
    expect(runMatchesFilter(alpha, f)).toBe(true);
    expect(runMatchesFilter(alpha, { ...f, pipeline: "other" })).toBe(false);
    expect(runMatchesFilter(alpha, { ...f, repo: "/repos/beta" })).toBe(false);
  });

  it("isFilterActive reports exactly the three axes", () => {
    expect(isFilterActive(EMPTY_RUN_FILTER)).toBe(false);
    expect(isFilterActive({ ...EMPTY_RUN_FILTER, repo: "/repos/x" })).toBe(true);
    expect(isFilterActive({ ...EMPTY_RUN_FILTER, pipeline: "p" })).toBe(true);
    expect(isFilterActive({ ...EMPTY_RUN_FILTER, trigger: "t" })).toBe(true);
  });
});
