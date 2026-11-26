import { describe, expect, it } from "vitest";

import {
  classifyCleanup,
  classifyForget,
  formatResidue,
  gitResidue,
  isReapable,
  ownedRunIds,
  parseBranchList,
  parseLedger,
  parseWorktreePorcelain,
  type GitSnapshot,
} from "./run-ledger-core";

const RUNS_DIR = "/repo/.pdo/runs";

function snap(branches: string[], worktrees: { path: string; prunable?: boolean }[]): GitSnapshot {
  return {
    branches: new Set(branches),
    worktrees: new Map(worktrees.map((w) => [w.path, { path: w.path, prunable: !!w.prunable }])),
  };
}

describe("ownedRunIds", () => {
  it("owns what appeared since the snapshot, never a pre-existing run", () => {
    const owned = ownedRunIds(new Set(["a", "b"]), ["a", "b", "c", "d"], []);
    expect([...owned].sort()).toEqual(["c", "d"]);
  });

  it("adds the runs the specs reaped themselves (ledger), still never a pre-existing one", () => {
    const owned = ownedRunIds(new Set(["a"]), ["a", "c"], ["e", "a"]);
    expect([...owned].sort()).toEqual(["c", "e"]);
  });
});

describe("gitResidue", () => {
  it("reports only the new branches of owned runs", () => {
    const before = snap(["pdo/run-old"], []);
    const after = snap(
      ["pdo/run-old", "pdo/run-own1", "pdo/sub-own1-worker", "pdo/run-foreign", "pdo/run-own10"],
      [],
    );
    const residue = gitResidue(before, after, new Set(["own1"]), RUNS_DIR);
    expect(residue.map((r) => [r.runId, r.kind, r.what])).toEqual([
      ["own1", "branch", "pdo/run-own1"],
      ["own1", "branch", "pdo/sub-own1-worker"],
    ]);
  });

  it("never reports a pre-existing branch, even of an owned id", () => {
    const before = snap(["pdo/run-own1"], []);
    const after = snap(["pdo/run-own1"], []);
    expect(gitResidue(before, after, new Set(["own1"]), RUNS_DIR)).toEqual([]);
  });

  it("reports new live and prunable worktrees under an owned run dir, not pre-existing ones", () => {
    const before = snap([], [{ path: `${RUNS_DIR}/old/worktree`, prunable: true }]);
    const after = snap(
      [],
      [
        { path: `${RUNS_DIR}/old/worktree`, prunable: true },
        { path: `${RUNS_DIR}/own1/worktree` },
        { path: `${RUNS_DIR}/own1/stray`, prunable: true },
        { path: `${RUNS_DIR}/foreign/worktree` },
        { path: "/elsewhere/.pdo/runs/own1/worktree" },
      ],
    );
    const residue = gitResidue(before, after, new Set(["own1"]), RUNS_DIR);
    expect(residue.map((r) => [r.runId, r.kind, r.what])).toEqual([
      ["own1", "prunable worktree", `${RUNS_DIR}/own1/stray`],
      ["own1", "worktree", `${RUNS_DIR}/own1/worktree`],
    ]);
  });
});

describe("classifyCleanup", () => {
  it("accepts 200 and an already archived run", () => {
    expect(classifyCleanup(200, "{}")).toBeNull();
    expect(classifyCleanup(409, '{"error":"run is already archived"}')).toBeNull();
  });

  it("fails on live children and on any other status", () => {
    expect(classifyCleanup(409, '{"error":"run has 1 live child run(s)","live_children":["x"]}')).toMatch(
      /409.*live child/,
    );
    expect(classifyCleanup(500, "boom")).toMatch(/500.*boom/);
    expect(classifyCleanup(404, "")).toMatch(/404/);
  });
});

describe("classifyForget", () => {
  it("accepts a 2xx and a run already gone", () => {
    expect(classifyForget(200, "")).toBeNull();
    expect(classifyForget(204, "")).toBeNull();
    expect(classifyForget(404, "")).toBeNull();
  });

  it("fails on anything else", () => {
    expect(classifyForget(409, "not archived")).toMatch(/409.*not archived/);
  });
});

describe("isReapable", () => {
  it("reaps a run of the e2e repo, of a temp repo, or with no target", () => {
    expect(isReapable(null, "/repo", "/tmp")).toBe(true);
    expect(isReapable("/repo", "/repo", "/tmp")).toBe(true);
    expect(isReapable("/repo/", "/repo", "/tmp")).toBe(true);
    expect(isReapable("/tmp/pdo336-alpha-x", "/repo", "/tmp")).toBe(true);
  });

  it("judges the canonical path, not the one the spec sent", () => {
    const canonical = (p: string) => p.replace(/^\/var\//, "/private/var/");
    expect(isReapable("/var/folders/x/pdo336-a", "/repo", "/private/var/folders", canonical)).toBe(true);
    expect(isReapable("/var/folders/x/pdo336-a", "/repo", "/private/var/folders")).toBe(false);
  });

  it("refuses any other repo", () => {
    expect(isReapable("/home/me/other", "/repo", "/tmp")).toBe(false);
    expect(isReapable("/repo-sibling", "/repo", "/tmp")).toBe(false);
    expect(isReapable("/tmpfoo/x", "/repo", "/tmp")).toBe(false);
  });
});

describe("parsers", () => {
  it("reads `git branch --format=%(refname:short)`", () => {
    expect(parseBranchList("pdo/run-a\n\npdo/sub-a-x\n")).toEqual(["pdo/run-a", "pdo/sub-a-x"]);
  });

  it("reads the ledger, pipeline optional", () => {
    expect(parseLedger("r1\tpipe-a\nr2\t\n\nr3\n")).toEqual(
      new Map([
        ["r1", "pipe-a"],
        ["r2", undefined],
        ["r3", undefined],
      ]),
    );
  });

  it("reads `git worktree list --porcelain`, prunable included", () => {
    const out = [
      "worktree /repo",
      "HEAD abc",
      "branch refs/heads/main",
      "",
      "worktree /repo/.pdo/runs/r1/worktree",
      "HEAD def",
      "branch refs/heads/pdo/run-r1",
      "prunable gitdir file points to non-existent location",
      "",
    ].join("\n");
    expect(parseWorktreePorcelain(out)).toEqual([
      { path: "/repo", prunable: false },
      { path: "/repo/.pdo/runs/r1/worktree", prunable: true },
    ]);
  });
});

describe("formatResidue", () => {
  it("names the run, its pipeline and each leftover", () => {
    const text = formatResidue(
      [
        { runId: "r1", kind: "branch", what: "pdo/run-r1" },
        { runId: "r1", kind: "run still listed", what: "status archived" },
      ],
      new Map([["r1", "e2e-initial-prompt"]]),
    );
    expect(text).toContain("r1 (e2e-initial-prompt)");
    expect(text).toContain("branch: pdo/run-r1");
    expect(text).toContain("run still listed: status archived");
  });
});
