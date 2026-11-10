import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { ciRunSteps, gateSteps } from "../lib/ci.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../..");
const CI = fs.readFileSync(path.join(root, ".github/workflows/ci.yml"), "utf8");

const WORKFLOW = `name: CI
jobs:
  rust:
    steps:
      - uses: actions/checkout@v4
      - run: cd frontend && pnpm install --frozen-lockfile
      # a comment
      - run: cargo nextest run --workspace
      - run: make types-check
  frontend:
    steps:
      - run: cd frontend && pnpm install --frozen-lockfile
      - run: cd frontend && pnpm run lint
  e2e:
    needs: frontend
    steps:
      - run: cd frontend && pnpm run e2e
        env:
          PDO_SKIP_FRONTEND_BUILD: "1"
`;

test("every `run:` step of every job, in order, the skipped jobs aside", () => {
  assert.deepEqual(ciRunSteps(WORKFLOW, { skipJobs: ["e2e"] }), [
    { job: "rust", command: "cd frontend && pnpm install --frozen-lockfile" },
    { job: "rust", command: "cargo nextest run --workspace" },
    { job: "rust", command: "make types-check" },
    { job: "frontend", command: "cd frontend && pnpm install --frozen-lockfile" },
    { job: "frontend", command: "cd frontend && pnpm run lint" },
  ]);
});

test("a step the gate cannot replay faithfully is refused, not guessed", () => {
  const multiline = "jobs:\n  rust:\n    steps:\n      - run: |\n          cargo test\n";
  assert.throws(() => ciRunSteps(multiline), /multi-line `run:`/);
  const workdir = "jobs:\n  rust:\n    steps:\n      - run: cargo test\n        working-directory: crates\n";
  assert.throws(() => ciRunSteps(workdir), /working-directory/);
  const env = "jobs:\n  rust:\n    steps:\n      - run: cargo test\n        env:\n          A: b\n";
  assert.throws(() => ciRunSteps(env), /env/);
});

test("a `run:` under a named step is read; a condition or a job default on it is refused", () => {
  const named = "jobs:\n  rust:\n    steps:\n      - name: Test\n        run: cargo test\n      - run: cargo fmt --check\n";
  assert.deepEqual(ciRunSteps(named).map((s) => s.command), ["cargo test", "cargo fmt --check"]);
  const cond = "jobs:\n  rust:\n    steps:\n      - name: Test\n        if: github.event_name == 'push'\n        run: cargo test\n";
  assert.throws(() => ciRunSteps(cond), /`cargo test` sets if/);
  const after = "jobs:\n  rust:\n    steps:\n      - run: cargo test\n        continue-on-error: true\n";
  assert.throws(() => ciRunSteps(after), /continue-on-error/);
  const defaults = "jobs:\n  rust:\n    defaults:\n      run:\n        working-directory: x\n    steps:\n      - run: cargo test\n";
  assert.throws(() => ciRunSteps(defaults), /defaults/);
  const jobEnv = "jobs:\n  rust:\n    env:\n      A: b\n    steps:\n      - run: cargo test\n";
  assert.throws(() => ciRunSteps(jobEnv), /job-level `env:`/);
  const usesWith = "jobs:\n  rust:\n    steps:\n      - uses: a/b@v1\n        with:\n          x: 1\n      - run: cargo test\n";
  assert.deepEqual(ciRunSteps(usesWith).map((s) => s.command), ["cargo test"]);
});

test("the gate runs each CI step once, nextest without fail-fast, then vitest and the script tests", () => {
  const steps = gateSteps(ciRunSteps(WORKFLOW, { skipJobs: ["e2e"] }));
  assert.deepEqual(
    steps.map((s) => s.command),
    [
      "cd frontend && pnpm install --frozen-lockfile",
      "cargo nextest run --workspace --no-fail-fast",
      "make types-check",
      "cd frontend && pnpm run lint",
      "cd frontend && pnpm test",
      "node --disable-warning=ExperimentalWarning --test 'scripts/*/test/*.test.mjs'",
    ],
  );
  assert.deepEqual(steps.at(-2), { name: "vitest", command: "cd frontend && pnpm test", source: "not in CI" });
  assert.equal(steps[1].source, "CI rust");
});

test("on the repository's CI: every declared step but the E2E job", () => {
  const commands = gateSteps(ciRunSteps(CI, { skipJobs: ["e2e"] })).map((s) => s.command);
  for (const expected of [
    "cargo check --workspace --all-targets",
    "cargo nextest run --workspace --no-fail-fast",
    "cargo test --workspace --doc",
    "make types-check",
    "cargo clippy --workspace --all-targets -- -D warnings",
    "cargo fmt --all --check",
    "bash tests/smoke.sh",
    "cd frontend && pnpm run typecheck",
    "cd frontend && pnpm run lint",
    "cd frontend && pnpm run build",
    "cd frontend && pnpm test",
  ]) {
    assert.ok(commands.includes(expected), `missing: ${expected}`);
  }
  assert.ok(!commands.some((c) => /playwright|pnpm run e2e|cargo build -p pdo-daemon/.test(c)), commands.join("\n"));
  assert.equal(new Set(commands).size, commands.length);
});
