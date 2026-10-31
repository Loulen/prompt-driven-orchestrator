import assert from "node:assert/strict";
import { test } from "node:test";
import { pickBase, planScoped } from "../lib/plan.mjs";

// A slice of the real workspace: kernel <- workspace <- skill, kernel <- platform,
// and the daemon on top of everything (tidy only as a dev-dependency).
const CRATES = [
  { name: "pdo-kernel", dir: "crates/pdo-kernel", deps: [] },
  { name: "pdo-workspace", dir: "crates/pdo-workspace", deps: ["pdo-kernel"] },
  { name: "pdo-skill", dir: "crates/pdo-skill", deps: ["pdo-kernel", "pdo-workspace"] },
  { name: "pdo-platform", dir: "crates/pdo-platform", deps: ["pdo-kernel"] },
  { name: "pdo-daemon", dir: "crates/pdo", deps: ["pdo-skill", "pdo-platform", "tidy"] },
  { name: "tidy", dir: "crates/tidy", deps: [] },
];

const plan = (changed, extra = {}) =>
  planScoped({
    changed,
    crates: CRATES,
    embeds: {},
    typeFiles: [],
    scriptSuites: ["scripts/release"],
    frontendInstalled: true,
    ...extra,
  });
const names = (p) => p.steps.map((s) => s.name);
const step = (p, name) => p.steps.find((s) => s.name === name);

test("a frontend-only diff keeps typecheck, lint and vitest, and compiles no crate", () => {
  const p = plan(["frontend/src/features/runs/RunList.tsx"]);
  assert.deepEqual(names(p), ["typecheck", "lint", "vitest"]);
  assert.ok(p.steps.every((s) => !/cargo/.test(s.command)));
  assert.match(step(p, "vitest").reason, /frontend\/src\/features\/runs\/RunList\.tsx/);
});

test("a fresh checkout installs the frontend dependencies first", () => {
  const p = plan(["frontend/src/a.ts"], { frontendInstalled: false });
  assert.deepEqual(names(p), ["frontend deps", "typecheck", "lint", "vitest"]);
  assert.match(step(p, "frontend deps").command, /pnpm install --frozen-lockfile/);
});

test("a crate nothing depends on runs nextest and clippy on that crate alone", () => {
  const p = plan(["crates/pdo-skill/src/lib.rs"]);
  assert.deepEqual(names(p), ["nextest", "clippy"]);
  assert.match(step(p, "nextest").command, /cargo nextest run -p pdo-daemon -p pdo-skill --no-fail-fast/);
  assert.match(step(p, "clippy").command, /cargo clippy -p pdo-daemon -p pdo-skill --all-targets -- -D warnings/);
  assert.match(step(p, "nextest").reason, /rdeps\(pdo-skill\)/);
});

test("a crate others depend on runs nextest and clippy on it and every dependent, transitively", () => {
  const p = plan(["crates/pdo-kernel/src/domain/id.rs"]);
  const expected = "-p pdo-daemon -p pdo-kernel -p pdo-platform -p pdo-skill -p pdo-workspace";
  assert.match(step(p, "nextest").command, new RegExp(`cargo nextest run ${expected} --no-fail-fast`));
  assert.match(step(p, "clippy").command, new RegExp(`cargo clippy ${expected} --all-targets`));
  assert.doesNotMatch(step(p, "nextest").command, /tidy/);
});

test("a changed file declaring a type exported to the front adds types-check", () => {
  const file = "crates/pdo-platform/src/domain/changelog.rs";
  const p = plan([file], { typeFiles: [file] });
  assert.deepEqual(names(p), ["types-check", "nextest", "clippy"]);
  assert.equal(step(p, "types-check").command, "make --no-print-directory types-check");
  assert.match(step(p, "types-check").reason, /changelog\.rs/);
});

test("an edited generated binding checks the bindings and the front that imports them", () => {
  const p = plan(["frontend/src/shared/generated/RunDetail.ts"]);
  assert.deepEqual(names(p), ["typecheck", "lint", "vitest", "types-check"]);
});

test("doc only adds nothing beyond the fast gate", () => {
  const p = plan(["docs/agents/git-flow.md", "CONTRIBUTING.md", ".agents/skills/implement/SKILL.md"]);
  assert.deepEqual(p.steps, []);
  assert.deepEqual(p.docs, ["docs/agents/git-flow.md", "CONTRIBUTING.md", ".agents/skills/implement/SKILL.md"]);
  assert.deepEqual(p.uncovered, []);
});

test("a file a crate embeds tests that crate, even when it looks like doc", () => {
  const p = plan(["CHANGELOG.md"], { embeds: { "CHANGELOG.md": ["pdo-platform"] } });
  assert.deepEqual(names(p), ["nextest", "clippy"]);
  assert.match(step(p, "nextest").command, /-p pdo-daemon -p pdo-platform --no-fail-fast/);
  assert.match(step(p, "nextest").reason, /CHANGELOG\.md, embedded by pdo-platform/);
  assert.deepEqual(p.docs, []);
});

test("a workspace manifest runs the whole workspace", () => {
  const p = plan(["Cargo.lock"]);
  assert.match(step(p, "nextest").command, /cargo nextest run --workspace --no-fail-fast/);
  assert.match(step(p, "clippy").command, /cargo clippy --workspace --all-targets -- -D warnings/);
});

test("a script with tests runs its node suite; what no rule covers is named, not run", () => {
  const p = plan(["scripts/release/lib/level.mjs", "Makefile", ".github/workflows/ci.yml"]);
  assert.deepEqual(names(p), ["scripts/release tests"]);
  assert.match(step(p, "scripts/release tests").command, /--test 'scripts\/release\/test\/\*\.test\.mjs'/);
  assert.deepEqual(p.uncovered, ["Makefile", ".github/workflows/ci.yml"]);
});

test("a mixed diff unions the steps, in a stable order, each once", () => {
  const p = plan(
    [
      "docs/README.md",
      "crates/pdo-workspace/src/a.rs",
      "frontend/src/b.tsx",
      "crates/pdo-skill/src/c.rs",
      "frontend/src/d.ts",
      "scripts/release/release-bump.mjs",
    ],
    { typeFiles: ["crates/pdo-workspace/src/a.rs"] },
  );
  assert.deepEqual(names(p), ["typecheck", "lint", "vitest", "types-check", "nextest", "clippy", "scripts/release tests"]);
  assert.match(step(p, "nextest").command, /-p pdo-daemon -p pdo-skill -p pdo-workspace --no-fail-fast/);
  assert.match(step(p, "nextest").reason, /rdeps\(pdo-skill\).*rdeps\(pdo-workspace\)|rdeps\(pdo-workspace\).*rdeps\(pdo-skill\)/);
  assert.match(step(p, "vitest").reason, /frontend\/src\/b\.tsx, frontend\/src\/d\.ts/);
  assert.deepEqual(p.docs, ["docs/README.md"]);
});

test("an empty diff plans nothing", () => {
  assert.deepEqual(plan([]), { steps: [], docs: [], uncovered: [] });
});

test("the base is the nearest branch the current one started from, never one that contains it", () => {
  const base = pickBase([
    { ref: "main", ahead: 3, behind: 40 },
    { ref: "integration/CORE-1-x", ahead: 0, behind: 2 },
    { ref: "feature/CORE-2-y", ahead: 5, behind: 4 },
    { ref: "fp/throwaway", ahead: 1, behind: 0 },
  ]);
  assert.equal(base, "integration/CORE-1-x");
});

test("a branch at the very same commit is a base: only the uncommitted changes remain", () => {
  assert.equal(pickBase([{ ref: "main", ahead: 0, behind: 9 }, { ref: "twin", ahead: 0, behind: 0 }]), "twin");
});

test("between two equally near branches, the one with fewer commits of its own wins", () => {
  assert.equal(
    pickBase([
      { ref: "b-busy", ahead: 7, behind: 2 },
      { ref: "a-quiet", ahead: 1, behind: 2 },
    ]),
    "a-quiet",
  );
  assert.equal(pickBase([]), null);
});
