import assert from "node:assert/strict";
import { test } from "node:test";
import { apiLevel, bump, proposeLevel } from "../lib/level.mjs";

const commit = (message, files = []) => ({ message, files });

test("a feat is a minor, a fix a patch, a `!` or a BREAKING CHANGE footer a major", () => {
  assert.equal(proposeLevel([commit("fix: a"), commit("docs: b")]).level, "patch");
  assert.equal(proposeLevel([commit("fix(run): a"), commit("feat(ui): b")]).level, "minor");
  assert.equal(proposeLevel([commit("feat(ui)!: b"), commit("fix: a")]).level, "major");
  assert.equal(proposeLevel([commit("refactor!: drop a field")]).level, "major");
  assert.equal(
    proposeLevel([commit("feat: a\n\nBREAKING CHANGE: the field is gone")]).level,
    "major",
  );
});

test("a commit outside feat and fix is still a patch: a push to main is a release", () => {
  assert.equal(proposeLevel([commit("docs: an ADR"), commit("chore: tidy")]).level, "patch");
  assert.equal(proposeLevel([commit("Some free-form subject")]).level, "patch");
});

test("merge commits weigh nothing, and no commit proposes no level", () => {
  assert.equal(proposeLevel([commit("Merge CORE-1 — feat: whatever")]).level, null);
  assert.equal(proposeLevel([commit("Merge branch 'x' into y")]).level, null);
  assert.equal(proposeLevel([]).level, null);
});

test("the proposal names the commits that decided it", () => {
  const { reasons } = proposeLevel([commit("fix: a"), commit("feat(ui): the new tab"), commit("feat: b")]);
  assert.deepEqual(reasons, ["feat(ui): the new tab", "feat: b"]);
});

test("bump resets what is below the level", () => {
  assert.equal(bump("2.68.3", "patch"), "2.68.4");
  assert.equal(bump("2.68.3", "minor"), "2.69.0");
  assert.equal(bump("2.68.3", "major"), "3.0.0");
  assert.throws(() => bump("2.68", "patch"), /not a version/);
  assert.throws(() => bump("2.68.0", "huge"), /unknown level/);
});

test("the API takes the level of the commits that touch it, a major being a minor in 0.x", () => {
  const touching = [
    commit("feat(ext)!: a location removed", ["crates/pdo-extension-api/src/ui/location.rs"]),
    commit("fix: elsewhere", ["crates/pdo-run/src/lib.rs"]),
  ];
  assert.equal(apiLevel(touching, "0.20.0").level, "minor");
  assert.equal(apiLevel(touching, "1.4.0").level, "major");
  const shared = [commit("feat: a primitive", ["frontend/src/shared/extensions/kit.ts"])];
  assert.equal(apiLevel(shared, "0.20.0").level, "minor");
  const tokens = [commit("fix: a token", ["frontend/public/shared/tokens.css"])];
  assert.equal(apiLevel(tokens, "0.20.0").level, "patch");
});

test("an API no commit touches still moves by a patch, which every extension accepts", () => {
  const elsewhere = [commit("feat!: a new screen", ["frontend/src/run/Canvas.tsx"])];
  assert.deepEqual(apiLevel(elsewhere, "0.20.0"), { level: "patch", reasons: [] });
});
