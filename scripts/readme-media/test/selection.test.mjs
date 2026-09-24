import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { after, test } from "node:test";
import { fileURLToPath } from "node:url";
import { loadScenes } from "../lib/scenes.mjs";
import { parseSelection, publish, readSelection } from "../lib/selection.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));

test("parses one `<scene> <variant>` per line, comments and blanks ignored", () => {
  const selection = parseSelection("# header\n\nstats  a   # published\nhero b\n");
  assert.deepEqual([...selection], [
    ["stats", "a"],
    ["hero", "b"],
  ]);
  assert.throws(() => parseSelection("stats\n"), /expected "<scene> <variant>"/);
  assert.throws(() => parseSelection("stats a\nstats b\n"), /selected twice/);
});

test("the versioned selection names every README scene, and a variant each built scene declares", async () => {
  const selection = readSelection(path.join(here, "..", "selection.txt"));
  assert.deepEqual([...selection.keys()].sort(), ["hero", "interactive-orchestrator", "outputs", "pipelines", "profiles", "review", "routing", "skills", "stats", "triggers"]);
  const scenes = await loadScenes(path.join(here, "..", "scenes"));
  for (const [name, scene] of scenes) {
    assert.ok(selection.has(name), `${name} has a selection line`);
    assert.ok(scene.variants.some((v) => v.id === selection.get(name)), `${name}: selected variant exists`);
  }
});

const roots = [];
after(() => roots.forEach((root) => fs.rmSync(root, { recursive: true, force: true })));

function fakeReview() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "readme-media-publish-"));
  roots.push(root);
  const reviewDir = path.join(root, "review");
  const assetsDir = path.join(root, "assets");
  for (const variant of ["a", "b"]) {
    fs.mkdirSync(path.join(reviewDir, "stats"), { recursive: true });
    fs.writeFileSync(path.join(reviewDir, "stats", `${variant}.gif`), `GIF of ${variant}`);
    fs.writeFileSync(path.join(reviewDir, "stats", `${variant}.jpg`), `JPG of ${variant}`);
  }
  const scenes = new Map([["stats", { name: "stats", variants: [{ id: "a" }, { id: "b" }] }]]);
  return { root, reviewDir, assetsDir, scenes };
}

test("publishes only the selected variant; changing the selection swaps it without recording", () => {
  const { reviewDir, assetsDir, scenes } = fakeReview();
  const log = () => {};
  const first = publish({ selection: parseSelection("stats a\nhero a\n"), scenes, reviewDir, assetsDir, log });
  assert.deepEqual(first.published, [{ scene: "stats", variant: "a" }]);
  assert.deepEqual(first.skipped, [{ scene: "hero", reason: "no scene file yet" }]);
  assert.deepEqual(fs.readdirSync(assetsDir).sort(), ["stats.gif", "stats.jpg"]);
  assert.equal(fs.readFileSync(path.join(assetsDir, "stats.gif"), "utf8"), "GIF of a");

  publish({ selection: parseSelection("stats b\n"), scenes, reviewDir, assetsDir, log });
  assert.equal(fs.readFileSync(path.join(assetsDir, "stats.gif"), "utf8"), "GIF of b");
  assert.equal(fs.readFileSync(path.join(assetsDir, "stats.jpg"), "utf8"), "JPG of b");
});

test("refuses a variant the scene does not declare, or one never recorded", () => {
  const { reviewDir, assetsDir, scenes } = fakeReview();
  assert.throws(() => publish({ selection: parseSelection("stats c\n"), scenes, reviewDir, assetsDir, log: () => {} }), /no variant "c"/);
  fs.rmSync(path.join(reviewDir, "stats", "b.gif"));
  assert.throws(() => publish({ selection: parseSelection("stats b\n"), scenes, reviewDir, assetsDir, log: () => {} }), /never recorded/);
});
