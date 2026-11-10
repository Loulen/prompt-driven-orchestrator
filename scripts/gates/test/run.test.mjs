import assert from "node:assert/strict";
import { test } from "node:test";
import { failureLines, runSteps } from "../lib/run.mjs";

const fakeExec = (results) => async (step) => ({ code: results[step.name]?.code ?? 0, output: results[step.name]?.output ?? "" });

test("a failed step does not stop the gate; the summary lists every failure, with its lines", async () => {
  const lines = [];
  const steps = [
    { name: "nextest", command: "cargo nextest run", reason: "r" },
    { name: "clippy", command: "cargo clippy", reason: "r" },
    { name: "vitest", command: "pnpm test", reason: "r" },
  ];
  const ran = [];
  const exec = async (step) => {
    ran.push(step.name);
    return fakeExec({
      nextest: { code: 100, output: "    PASS [0.1s] a\n        FAIL [   0.010s] pdo-kernel id::tests::broken\n" },
      vitest: { code: 1, output: " ✓ ok\n FAIL  src/a.test.ts > a > breaks\n" },
    })(step);
  };
  const code = await runSteps(steps, { exec, log: (l) => lines.push(l), now: () => 0 });
  assert.equal(code, 1);
  assert.deepEqual(ran, ["nextest", "clippy", "vitest"]);
  const summary = lines.slice(lines.findIndex((l) => /^2 of 3 steps failed/.test(l))).join("\n");
  assert.match(summary, /FAILED nextest \(exit 100\)/);
  assert.match(summary, /FAIL \[ {3}0\.010s\] pdo-kernel id::tests::broken/);
  assert.match(summary, /FAILED vitest \(exit 1\)/);
  assert.match(summary, /FAIL {2}src\/a\.test\.ts > a > breaks/);
  assert.doesNotMatch(summary, /clippy/);
});

test("every step is announced with its command and reason, then timed", async () => {
  const lines = [];
  let t = 0;
  const now = () => (t += 1500);
  const code = await runSteps([{ name: "lint", command: "pnpm run lint", reason: "frontend changed" }], {
    exec: fakeExec({}),
    log: (l) => lines.push(l),
    now,
  });
  assert.equal(code, 0);
  assert.ok(lines.some((l) => l.includes("[1/1] lint") && l.includes("pnpm run lint")));
  assert.ok(lines.some((l) => l.includes("frontend changed")));
  assert.ok(lines.some((l) => /ok lint in 1\.5 s/.test(l)));
  assert.ok(lines.some((l) => /^all 1 steps passed/.test(l)));
});

test("the failure lines of the usual tools, colours stripped, capped; the tail otherwise", () => {
  const nextest = "\u001b[31m        FAIL\u001b[0m [   0.01s] pdo-run tests::a\n  PASS b\nerror: test run failed\n";
  assert.deepEqual(failureLines(nextest), ["        FAIL [   0.01s] pdo-run tests::a", "error: test run failed"]);
  const rustc = "warning: unused\nerror[E0308]: mismatched types\n --> src/a.rs:1:1\n";
  assert.deepEqual(failureLines(rustc), ["error[E0308]: mismatched types"]);
  const vitest = " × breaks 3ms\n ✓ ok\n";
  assert.deepEqual(failureLines(vitest), [" × breaks 3ms"]);
  const node = "not ok 2 - a test\n# fail 1\n";
  assert.deepEqual(failureLines(node), ["not ok 2 - a test"]);
  const silent = Array.from({ length: 20 }, (_, i) => `line ${i}`).join("\n");
  assert.deepEqual(failureLines(silent), ["line 15", "line 16", "line 17", "line 18", "line 19"]);
  const many = Array.from({ length: 50 }, (_, i) => `FAIL t${i}`).join("\n");
  assert.equal(failureLines(many).length, 21);
  assert.match(failureLines(many).at(-1), /\+30 more/);
});
