import assert from "node:assert/strict";
import { test } from "node:test";
import { releaseChangelog } from "../lib/changelog.mjs";

const PREAMBLE = "# Changelog\n\nOne entry per version.\n\n";

test("Unreleased becomes the version's heading, its title kept", () => {
  const text = `${PREAMBLE}## Unreleased · Faster gates\n\n- one\n\n## 2.68.0 · Themes\n\n- old\n`;
  assert.equal(
    releaseChangelog(text, "2.69.0"),
    `${PREAMBLE}## 2.69.0 · Faster gates\n\n- one\n\n## 2.68.0 · Themes\n\n- old\n`,
  );
});

test("an Unreleased without a title gives a bare version heading", () => {
  const text = `${PREAMBLE}## Unreleased\n\n- one\n\n## 2.68.0\n`;
  assert.equal(releaseChangelog(text, "3.0.0"), `${PREAMBLE}## 3.0.0\n\n- one\n\n## 2.68.0\n`);
});

test("only the heading changes: an `Unreleased` in a body stays", () => {
  const text = `${PREAMBLE}## Unreleased\n\n- the Unreleased section is hidden\n\n## 2.68.0\n`;
  assert.match(releaseChangelog(text, "2.69.0"), /- the Unreleased section is hidden/);
});

test("a changelog without Unreleased, or with it below a version, is refused", () => {
  assert.throws(() => releaseChangelog(`${PREAMBLE}## 2.68.0\n`, "2.69.0"), /no `## Unreleased`/);
  assert.throws(
    () => releaseChangelog(`${PREAMBLE}## 2.68.0\n\n## Unreleased\n`, "2.69.0"),
    /must be the first entry/,
  );
});

test("an empty Unreleased is refused: a release says what it brings", () => {
  assert.throws(
    () => releaseChangelog(`${PREAMBLE}## Unreleased\n\n## 2.68.0\n`, "2.69.0"),
    /Unreleased is empty/,
  );
});
