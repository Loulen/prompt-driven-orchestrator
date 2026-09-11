// #783 — unit tests for the pure run-tree model behind the Runs list.
import { describe, expect, it } from "vitest";
import type { RunListEntry } from "../types";
import {
  ancestorIds,
  buildRunTree,
  filterRunTree,
  flattenAll,
  flattenVisible,
  parentIds,
  pathOnlyIds,
  rootOf,
  runCountBucket,
} from "./runTree";

const run = (id: string, over: Partial<RunListEntry> = {}): RunListEntry => ({
  run_id: id,
  pipeline_name: "p",
  status: "running",
  started_at: null,
  ...over,
});

const list: RunListEntry[] = [
  run("epic"),
  run("kid", { parent_run_id: "epic", stalled: true }),
  run("kid2", { parent_run_id: "epic", status: "failed", pipeline_name: "impl" }),
  run("grandkid", { parent_run_id: "kid", status: "completed", pipeline_name: "review" }),
  run("orphan", { parent_run_id: "forgotten", status: "halted" }),
  run("solo", { status: "archived" }),
];

describe("buildRunTree", () => {
  it("nests a child under a LISTED parent and keeps the list order among siblings", () => {
    const forest = buildRunTree(list);
    expect(forest.map((n) => n.run.run_id)).toEqual(["epic", "orphan", "solo"]);
    const epic = forest[0];
    expect(epic.children.map((n) => n.run.run_id)).toEqual(["kid", "kid2"]);
    expect(epic.children[0].children.map((n) => n.run.run_id)).toEqual(["grandkid"]);
    expect(epic.depth).toBe(0);
    expect(epic.children[0].depth).toBe(1);
    expect(epic.children[0].children[0].depth).toBe(2);
  });

  it("makes a run whose parent is absent (forgotten / never listed) a root", () => {
    const forest = buildRunTree(list);
    expect(forest.find((n) => n.run.run_id === "orphan")?.depth).toBe(0);
  });

  it("aggregates the four disjoint counts over the WHOLE subtree", () => {
    const [epic] = buildRunTree(list);
    // kid (live+stalled ⇒ stale), kid2 (failed), grandkid (completed ⇒ finished)
    expect(epic.counts).toEqual({ finished: 1, failed: 1, stale: 1, running: 0 });
    expect(epic.children[0].counts).toEqual({ finished: 1, failed: 0, stale: 0, running: 0 });
    expect(epic.children[1].counts).toEqual({ finished: 0, failed: 0, stale: 0, running: 0 });
  });

  it("never loses a run to a corrupted parent cycle", () => {
    const cyclic = [run("a", { parent_run_id: "b" }), run("b", { parent_run_id: "a" })];
    const ids = flattenAll(buildRunTree(cyclic)).map((r) => r.run_id).sort();
    expect(ids).toEqual(["a", "b"]);
  });
});

describe("runCountBucket", () => {
  it("buckets by status and stalled: finished / failed / stale / running", () => {
    expect(runCountBucket(run("x", { status: "completed" }))).toBe("finished");
    expect(runCountBucket(run("x", { status: "halted" }))).toBe("finished");
    expect(runCountBucket(run("x", { status: "skipped" }))).toBe("finished");
    expect(runCountBucket(run("x", { status: "archived" }))).toBe("finished");
    expect(runCountBucket(run("x", { status: "failed" }))).toBe("failed");
    expect(runCountBucket(run("x", { status: "failed", stalled: true }))).toBe("failed");
    expect(runCountBucket(run("x", { status: "running", stalled: true }))).toBe("stale");
    expect(runCountBucket(run("x", { status: "awaiting_user", stalled: true }))).toBe("stale");
    expect(runCountBucket(run("x", { status: "running" }))).toBe("running");
    expect(runCountBucket(run("x", { status: "awaiting_user" }))).toBe("running");
    expect(runCountBucket(run("x", { status: "paused" }))).toBe("running");
  });
});

describe("filterRunTree", () => {
  it("keeps a non-matching parent as the path to a matching descendant and drops non-matching siblings", () => {
    const forest = filterRunTree(buildRunTree(list), (r) => r.pipeline_name === "review");
    expect(forest.map((n) => n.run.run_id)).toEqual(["epic"]);
    expect(forest[0].children.map((n) => n.run.run_id)).toEqual(["kid"]);
    expect(forest[0].children[0].children.map((n) => n.run.run_id)).toEqual(["grandkid"]);
  });

  it("does NOT recompute the counts — the pills describe the real subtree", () => {
    const forest = filterRunTree(buildRunTree(list), (r) => r.pipeline_name === "review");
    expect(forest[0].counts).toEqual({ finished: 1, failed: 1, stale: 1, running: 0 });
  });

  it("a matching parent keeps only its matching children", () => {
    const forest = filterRunTree(buildRunTree(list), (r) => r.pipeline_name === "p");
    const epic = forest.find((n) => n.run.run_id === "epic")!;
    expect(epic.children.map((n) => n.run.run_id)).toEqual(["kid"]); // kid2 is "impl"
    expect(epic.children[0].children).toEqual([]); // grandkid is "review"
  });

  it("returns nothing when nothing matches", () => {
    expect(filterRunTree(buildRunTree(list), () => false)).toEqual([]);
  });

  it("pathOnlyIds names the parents kept only as a path", () => {
    const match = (r: RunListEntry) => r.pipeline_name === "review";
    const forest = filterRunTree(buildRunTree(list), match);
    expect(pathOnlyIds(forest, match)).toEqual(["epic", "kid"]);
  });
});

describe("parentIds / ancestorIds / rootOf", () => {
  it("lists every node with children, depth-first", () => {
    expect(parentIds(buildRunTree(list))).toEqual(["epic", "kid"]);
  });

  it("walks ancestors nearest-first through LISTED parents only", () => {
    expect(ancestorIds(list, "grandkid")).toEqual(["kid", "epic"]);
    expect(ancestorIds(list, "epic")).toEqual([]);
    expect(ancestorIds(list, "orphan")).toEqual([]);
    expect(ancestorIds(list, "unknown")).toEqual([]);
  });

  it("resolves the root of a nested run (itself for a root)", () => {
    expect(rootOf(list, "grandkid")?.run_id).toBe("epic");
    expect(rootOf(list, "orphan")?.run_id).toBe("orphan");
    expect(rootOf(list, "nope")).toBeUndefined();
  });
});

describe("flattenVisible", () => {
  it("skips the subtree of a collapsed parent — the shift-range basis", () => {
    const forest = buildRunTree(list);
    const all = flattenVisible(forest, () => true).map((n) => n.run.run_id);
    expect(all).toEqual(["epic", "kid", "grandkid", "kid2", "orphan", "solo"]);
    const kidClosed = flattenVisible(forest, (id) => id !== "kid").map((n) => n.run.run_id);
    expect(kidClosed).toEqual(["epic", "kid", "kid2", "orphan", "solo"]);
    const epicClosed = flattenVisible(forest, (id) => id !== "epic").map((n) => n.run.run_id);
    expect(epicClosed).toEqual(["epic", "orphan", "solo"]);
  });
});
