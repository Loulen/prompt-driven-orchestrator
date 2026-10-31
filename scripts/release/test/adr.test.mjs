import assert from "node:assert/strict";
import { test } from "node:test";
import { numberDrafts, rewriteCitations } from "../lib/adr.mjs";

const ADRS = [
  "0092-une-extension.md",
  "0093-le-theme.md",
  "draft-version-se-fixe.md",
  "draft-les-controles.md",
  "README.md",
];

test("drafts are numbered after the highest number, in name order", () => {
  assert.deepEqual(numberDrafts(ADRS), [
    { slug: "les-controles", id: "ADR-0094", from: "draft-les-controles.md", to: "0094-les-controles.md" },
    { slug: "version-se-fixe", id: "ADR-0095", from: "draft-version-se-fixe.md", to: "0095-version-se-fixe.md" },
  ]);
});

test("no draft, no rename; a first ADR is 0001", () => {
  assert.deepEqual(numberDrafts(["0001-a.md"]), []);
  assert.equal(numberDrafts(["draft-a.md"])[0].to, "0001-a.md");
});

test("a draft whose number is taken by a slug already numbered is refused", () => {
  assert.throws(() => numberDrafts(["0093-a.md", "draft-a.md"]), /already numbered/);
});

const renames = numberDrafts(ADRS);

test("every form of a citation is rewritten: ADR name, link, path", () => {
  const text = [
    "Decided in ADR draft-version-se-fixe, see ADR `draft-les-controles`.",
    "[the ADR](docs/adr/draft-version-se-fixe.md), [sibling](./draft-les-controles.md)",
    "(*terme*, ADR draft-les-controles) and `draft-version-se-fixe` alone.",
  ].join("\n");
  assert.equal(
    rewriteCitations(text, renames),
    [
      "Decided in ADR-0095, see ADR-0094.",
      "[the ADR](docs/adr/0095-version-se-fixe.md), [sibling](./0094-les-controles.md)",
      "(*terme*, ADR-0094) and ADR-0095 alone.",
    ].join("\n"),
  );
});

test("a longer slug sharing a draft's prefix is not touched", () => {
  const text = "ADR draft-les-controles-bis and draft-target";
  assert.equal(rewriteCitations(text, renames), text);
});

test("a text citing no draft is returned unchanged", () => {
  const text = "Amends ADR-0093.";
  assert.equal(rewriteCitations(text, renames), text);
});
