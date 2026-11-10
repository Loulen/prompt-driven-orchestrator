// The sequence of one bench, its effects faked: worktree, measures, cleanup.

import assert from "node:assert/strict";
import { test } from "node:test";
import { outcome, runBench } from "../lib/bench.mjs";

const BEFORE = "build:\n\tcargo build\ntest:\n\tcargo nextest run\n";
const AFTER = `${BEFORE}fast:\n\tx\nscoped:\n\tx\ngate:\n\tx\n`;

/** Fake ops recording what happened; `run` answers from `codes[id]` (0 by default). */
function fakeOps({ makefile = AFTER, codes = {}, outputs = {}, sccache = "sccache 0.19.0", free = 100, throwOn } = {}) {
  const calls = [];
  const ops = {
    now: () => "2026-10-31T00:00:00Z",
    load: () => 2,
    freeGb: () => free,
    sccache: () => sccache,
    readMakefile: () => makefile,
    addWorktree: async (ref) => (calls.push(`add ${ref}`), { dir: "/tmp/wt", sha: "40e82f49aaaa" }),
    removeWorktree: async (dir) => calls.push(`remove ${dir}`),
    startCache: async () => calls.push("cache start"),
    stopCache: async () => calls.push("cache stop"),
    cacheStats: () => "sccache 3 hits / 0 misses",
    shell: async (_dir, line) => (calls.push(`shell ${line.split("\n")[0]}`), 0),
    run: async (_dir, m) => {
      calls.push(`run ${m.id}`);
      if (throwOn === m.id) throw new Error("boom");
      return { code: codes[m.id] ?? 0, seconds: 10, output: outputs[m.id] ?? "" };
    },
  };
  return { ops, calls };
}

test("a ref older than the story: the missing gates are « n/a », nothing fails", async () => {
  const { ops, calls } = fakeOps({
    makefile: BEFORE,
    codes: { structural: 100 },
    outputs: { structural: "FAIL pdo::it file_size: crates/pdo/src/bench_oversized.rs: 401 lines" },
  });
  const bench = await runBench("40e82f49", { ops, log: () => {} });
  const by = Object.fromEntries(bench.measures.map((m) => [m.id, m]));
  assert.equal(by["scoped-front"].na, "n/a (no `make scoped` at this ref)");
  assert.equal(by.structural.outcome, "caught");
  assert.equal(by.structural.command, "make test");
  assert.equal(by["full-suite"].command, "make test");
  assert.equal(by["full-suite"].outcome, "ok");
  assert.ok(!calls.includes("run scoped-front"));
  assert.equal(calls.at(-1), "remove /tmp/wt");
});

test("the worktree and the cache are removed when a measure throws", async () => {
  const { ops, calls } = fakeOps({ throwOn: "cold-build" });
  await assert.rejects(runBench("HEAD", { ops, log: () => {} }), /boom/);
  assert.deepEqual(calls.slice(-2), ["cache stop", "remove /tmp/wt"]);
});

test("a setup that fails stops the bench, the worktree removed all the same", async () => {
  const { ops, calls } = fakeOps();
  ops.shell = async () => 1;
  await assert.rejects(runBench("HEAD", { ops, log: () => {} }), /structural: setup failed/);
  assert.equal(calls.at(-1), "remove /tmp/wt");
});

test("below the free disk needed, the measures left are skipped, never run", async () => {
  const { ops, calls } = fakeOps({ free: 10 });
  const bench = await runBench("HEAD", { ops, minFreeGb: 25, log: () => {} });
  assert.ok(bench.measures.filter((m) => !m.na).every((m) => /^10 GB free, 25 needed$/.test(m.skipped)));
  assert.ok(!calls.some((c) => c.startsWith("run ")));
});

test("the wrapped builds carry the sccache stats and the load when they started", async () => {
  const { ops } = fakeOps();
  const bench = await runBench("HEAD", { ops, log: () => {} });
  const warm = bench.measures.find((m) => m.id === "warm-build");
  assert.equal(warm.note, "sccache 3 hits / 0 misses");
  assert.equal(warm.load, 2);
  assert.equal(bench.measures.find((m) => m.id === "full-suite").note, undefined);
});

test("an injected failure is « caught » only when the output names the file", () => {
  const m = { expect: "fail", marker: "bench_oversized" };
  assert.equal(outcome(m, 101, true), "caught");
  assert.equal(outcome(m, 101, false), "failed for another reason (exit 101)");
  assert.equal(outcome(m, 0, false), "not caught");
  assert.equal(outcome({ expect: "pass" }, 0, false), "ok");
  assert.equal(outcome({ expect: "pass" }, 2, false), "failed (exit 2)");
});
