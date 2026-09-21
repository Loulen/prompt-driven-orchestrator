import { describe, it, expect } from "vitest";
import {
  deliveryOfPair,
  defaultCollapsed,
  fileHunks,
  groupByDir,
  hunkToUnified,
  isDefaultPair,
  miniBar,
  nodeReviewTarget,
  pairFromSearch,
  pairOfDelivery,
  readView,
  reconcilePair,
  reviewRunIdFromPath,
  reviewUrl,
  shortSha,
  statusLetter,
  writeView,
  deliverySignature,
  defaultPair,
  hasExplicitPair,
} from "./runRefs";
import type { DiffFile, RunRefs } from "../types";

const REFS: RunRefs = {
  default_from: "fork",
  default_to: "tip",
  refs: [
    { id: "fork", kind: "fork", label: "Fork point", git_ref: "aaaa", sha: "aaaa" },
    { id: "node:impl:1:before", kind: "before", label: "implement · iter 1 · before", git_ref: "aaaa", sha: "aaaa", node_id: "impl", node_name: "implement", iter: 1 },
    { id: "node:impl:1:after", kind: "after", label: "implement · iter 1 · after", git_ref: "bbbb", sha: "bbbb", node_id: "impl", node_name: "implement", iter: 1 },
    { id: "live:rev", kind: "live", label: "review · iter 1 · live", git_ref: "pdo/sub-x-rev-iter-1", sha: "cccc", node_id: "rev", node_name: "review", iter: 1 },
    { id: "tip", kind: "tip", label: "Run tip", git_ref: "pdo/run-x", sha: "bbbb" },
  ],
  deliveries: [
    { node_id: "impl", node_name: "implement", iter: 1, status: "delivered", before: "node:impl:1:before", after: "node:impl:1:after" },
    { node_id: "rev", node_name: "review", iter: 1, status: "running", before: "tip", live: "live:rev" },
  ],
};

function file(overrides: Partial<DiffFile> & { path: string }): DiffFile {
  const { path, ...rest } = overrides;
  return {
    old_path: path,
    new_path: path,
    status: "modified",
    binary: false,
    additions: 1,
    deletions: 1,
    hunks: [],
    ...rest,
  };
}

describe("Review URL contract (#749)", () => {
  it("recognises /runs/<id>/review and nothing else", () => {
    expect(reviewRunIdFromPath("/runs/20260909-1259-abc/review")).toBe("20260909-1259-abc");
    expect(reviewRunIdFromPath("/runs/20260909-1259-abc/review/")).toBe("20260909-1259-abc");
    expect(reviewRunIdFromPath("/runs/a%20b/review")).toBe("a b");
    expect(reviewRunIdFromPath("/")).toBeNull();
    expect(reviewRunIdFromPath("/runs/abc")).toBeNull();
    expect(reviewRunIdFromPath("/runs/abc/review/extra")).toBeNull();
  });

  it("omits the default pair from the URL and carries any other pair as ids", () => {
    expect(reviewUrl("r1")).toBe("/runs/r1/review");
    expect(reviewUrl("r1", { from: "fork", to: "tip" })).toBe("/runs/r1/review");
    expect(reviewUrl("r1", { from: "node:impl:1:before", to: "node:impl:1:after" })).toBe(
      "/runs/r1/review?from=node%3Aimpl%3A1%3Abefore&to=node%3Aimpl%3A1%3Aafter",
    );
    expect(reviewUrl("r/1", { to: "live:rev" })).toBe("/runs/r%2F1/review?from=fork&to=live%3Arev");
  });

  it("reads a pair back from the query string with defaults", () => {
    expect(pairFromSearch("")).toEqual({ from: "fork", to: "tip" });
    expect(pairFromSearch("?from=node%3Aimpl%3A1%3Abefore&to=node%3Aimpl%3A1%3Aafter")).toEqual({
      from: "node:impl:1:before",
      to: "node:impl:1:after",
    });
    expect(pairFromSearch("?to=live:rev")).toEqual({ from: "fork", to: "live:rev" });
    expect(isDefaultPair(pairFromSearch(""))).toBe(true);
    expect(isDefaultPair({ from: "tip", to: "fork" })).toBe(false);
  });

  it("follows the daemon's default pair — fork → worktree while the Run's worktree exists (#835)", () => {
    const WT_REFS: RunRefs = {
      ...REFS,
      default_to: "worktree",
      refs: [...REFS.refs, { id: "worktree", kind: "worktree", label: "Working tree", git_ref: ".pdo/runs/x/worktree", sha: "7ee7" }],
    };
    const d = defaultPair(WT_REFS);
    expect(d).toEqual({ from: "fork", to: "worktree" });
    expect(defaultPair(null)).toEqual({ from: "fork", to: "tip" });
    expect(defaultPair(REFS)).toEqual({ from: "fork", to: "tip" });
    // The URL leaves the default out, whatever it is; tip spelled out is then explicit.
    expect(reviewUrl("r1", d, d)).toBe("/runs/r1/review");
    expect(reviewUrl("r1", { from: "fork", to: "tip" }, d)).toBe("/runs/r1/review?from=fork&to=tip");
    expect(pairFromSearch("", d)).toEqual(d);
    expect(pairFromSearch("?to=tip", d)).toEqual({ from: "fork", to: "tip" });
    expect(isDefaultPair(d, d)).toBe(true);
    expect(isDefaultPair({ from: "fork", to: "tip" }, d)).toBe(false);
    expect(hasExplicitPair("")).toBe(false);
    expect(hasExplicitPair("?to=tip")).toBe(true);
    // A bad side falls back to the daemon's default, not the hard-coded tip.
    expect(reconcilePair({ from: "fork", to: "ghost" }, WT_REFS).pair).toEqual(d);
  });

  it("falls back to the default side for an unknown ref, with a notice", () => {
    expect(reconcilePair({ from: "fork", to: "tip" }, REFS)).toEqual({
      pair: { from: "fork", to: "tip" },
      notice: null,
    });
    const r = reconcilePair({ from: "node:ghost:1:before", to: "node:impl:1:after" }, REFS);
    expect(r.pair).toEqual({ from: "fork", to: "node:impl:1:after" });
    expect(r.notice).toContain('"node:ghost:1:before"');
    const both = reconcilePair({ from: "x", to: "y" }, REFS);
    expect(both.pair).toEqual({ from: "fork", to: "tip" });
    expect(both.notice).toContain("Unknown refs");
  });
});

describe("delivery pairs", () => {
  it("recognises a pair that is exactly one node's delivery (or live branch)", () => {
    expect(deliveryOfPair({ from: "node:impl:1:before", to: "node:impl:1:after" }, REFS)?.node_name).toBe(
      "implement",
    );
    expect(deliveryOfPair({ from: "tip", to: "live:rev" }, REFS)?.node_name).toBe("review");
    expect(deliveryOfPair({ from: "fork", to: "tip" }, REFS)).toBeNull();
    expect(deliveryOfPair({ from: "fork", to: "node:impl:1:after" }, REFS)).toBeNull();
  });

  it("a delivery row selects before → after, or before → live while running", () => {
    expect(pairOfDelivery(REFS.deliveries[0])).toEqual({ from: "node:impl:1:before", to: "node:impl:1:after" });
    expect(pairOfDelivery(REFS.deliveries[1])).toEqual({ from: "tip", to: "live:rev" });
    expect(pairOfDelivery({ node_id: "n", node_name: "n", iter: 1, status: "running", before: "tip" })).toBeNull();
  });

  it("shortens a 40-char SHA and leaves branches alone", () => {
    expect(shortSha("0123456789abcdef0123456789abcdef01234567")).toBe("0123456");
    expect(shortSha("pdo/sub-x")).toBe("pdo/sub-x");
    expect(shortSha(null)).toBe("");
  });
});

describe("node panel shortcut (#749 AC7)", () => {
  it("opens before → after for a node with a recorded delivery, whatever its status", () => {
    const t = nodeReviewTarget({
      node_id: "impl",
      iter: 2,
      status: "completed",
      delivery: { before: "a", after: "b" },
      isolated_worktree: false,
    });
    expect(t).toEqual({
      pair: { from: "node:impl:2:before", to: "node:impl:2:after" },
      label: "Review this node's delivery",
    });
  });

  it("opens tip → live for a running isolated node without a delivery yet", () => {
    const t = nodeReviewTarget({ node_id: "rev", iter: 1, status: "running", isolated_worktree: true });
    expect(t).toEqual({ pair: { from: "tip", to: "live:rev" }, label: "Review live changes" });
    expect(nodeReviewTarget({ node_id: "rev", iter: 1, status: "awaiting_user", isolated_worktree: true })?.label).toBe(
      "Review live changes",
    );
  });

  it("offers nothing for a node with nothing to review", () => {
    expect(nodeReviewTarget({ node_id: "p", iter: 1, status: "pending" })).toBeNull();
    expect(nodeReviewTarget({ node_id: "p", iter: 1, status: "skipped" })).toBeNull();
    expect(nodeReviewTarget({ node_id: "p", iter: 1, status: "failed", delivery: null })).toBeNull();
    // Running but sharing the Run's worktree: no live branch of its own.
    expect(nodeReviewTarget({ node_id: "p", iter: 1, status: "running", isolated_worktree: false })).toBeNull();
    expect(nodeReviewTarget({ node_id: "p", iter: 1, status: "running" })).toBeNull();
  });
});

describe("structured hunks → the diff component", () => {
  it("re-serialises a hunk as unified text without parsing anything", () => {
    const text = hunkToUnified({
      old_start: 3,
      old_lines: 2,
      new_start: 3,
      new_lines: 3,
      header: "fn main()",
      lines: [
        { kind: "context", content: "a", old_no: 3, new_no: 3 },
        { kind: "del", content: "b", old_no: 4, new_no: null },
        { kind: "add", content: "B", old_no: null, new_no: 4 },
        { kind: "add", content: "", old_no: null, new_no: 5 },
      ],
    });
    expect(text).toBe("@@ -3,2 +3,3 @@ fn main()\n a\n-b\n+B\n+");
    expect(hunkToUnified({ old_start: 0, old_lines: 0, new_start: 1, new_lines: 1, header: "", lines: [] })).toBe(
      "@@ -0,0 +1,1 @@",
    );
  });

  it("maps every hunk of a file, each behind the ---/+++ header the parser needs", () => {
    const f = file({
      path: "a.rs",
      hunks: [
        { old_start: 1, old_lines: 1, new_start: 1, new_lines: 1, header: "", lines: [] },
        { old_start: 9, old_lines: 1, new_start: 9, new_lines: 1, header: "", lines: [] },
      ],
    });
    expect(fileHunks(f)).toEqual([
      "--- a/a.rs\n+++ b/a.rs\n@@ -1,1 +1,1 @@",
      "--- a/a.rs\n+++ b/a.rs\n@@ -9,1 +9,1 @@",
    ]);
    const added = file({
      path: "new.rs",
      old_path: null,
      status: "added",
      hunks: [{ old_start: 0, old_lines: 0, new_start: 1, new_lines: 1, header: "", lines: [] }],
    });
    expect(fileHunks(added)).toEqual(["--- /dev/null\n+++ b/new.rs\n@@ -0,0 +1,1 @@"]);
  });
});

describe("file list helpers", () => {
  it("letters the status", () => {
    expect(statusLetter(file({ path: "a", status: "added" }))).toBe("A");
    expect(statusLetter(file({ path: "a", status: "copied" }))).toBe("A");
    expect(statusLetter(file({ path: "a", status: "deleted" }))).toBe("D");
    expect(statusLetter(file({ path: "a", status: "renamed" }))).toBe("R");
    expect(statusLetter(file({ path: "a", status: "modified" }))).toBe("M");
  });

  it("groups files by directory in patch order, merging consecutive runs only", () => {
    const groups = groupByDir([
      file({ path: "src/a.ts" }),
      file({ path: "src/b.ts" }),
      file({ path: "README.md" }),
      file({ path: "src/c.ts" }),
    ]);
    expect(groups.map((g) => [g.dir, g.files.map((f) => f.index)])).toEqual([
      ["src", [0, 1]],
      ["", [2]],
      ["src", [3]],
    ]);
  });

  it("splits the mini bar between additions and deletions, empty for binaries and pure renames", () => {
    expect(miniBar(file({ path: "a", additions: 8, deletions: 2 }))).toEqual(["a", "a", "a", "a", "d"]);
    expect(miniBar(file({ path: "a", additions: 0, deletions: 3 }))).toEqual(["d", "d", "d", "d", "d"]);
    expect(miniBar(file({ path: "a", binary: true }))).toEqual(["", "", "", "", ""]);
    expect(miniBar(file({ path: "a", status: "renamed", additions: 0, deletions: 0 }))).toEqual(["", "", "", "", ""]);
  });

  it("starts collapsed only past the large-diff or large-file thresholds", () => {
    const small = [file({ path: "a" }), file({ path: "b" })];
    expect(defaultCollapsed(small).size).toBe(0);
    const huge = [file({ path: "a", additions: 1000, deletions: 600 }), file({ path: "b" })];
    expect([...defaultCollapsed(huge)]).toEqual(["a"]);
    const many = Array.from({ length: 41 }, (_, i) => file({ path: `f${i}` }));
    expect(defaultCollapsed(many).size).toBe(41);
  });
});

describe("browser persistence", () => {
  it("reads split by default and writes the toggle", () => {
    const store = new Map<string, string>();
    const storage = {
      getItem: (k: string) => store.get(k) ?? null,
      setItem: (k: string, v: string) => void store.set(k, v),
    };
    expect(readView(storage)).toBe("split");
    writeView("unified", storage);
    expect(readView(storage)).toBe("unified");
    store.set("pdo.review.view", "garbage");
    expect(readView(storage)).toBe("split");
  });

  it("signs the Run's deliveries so a moved tip is detectable", () => {
    expect(deliverySignature({ a: { delivery: { before: "x", after: "y" } }, b: {} })).toBe("y|");
    expect(deliverySignature({})).toBe("");
  });
});
