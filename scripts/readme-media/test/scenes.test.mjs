import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { loadScenes, selectScenes, validateScene } from "../lib/scenes.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));

test("every file of scenes/ is discovered, with two named variants and their markers", async () => {
  const scenes = await loadScenes(path.join(here, "..", "scenes"));
  assert.ok(scenes.has("stats"));
  const stats = scenes.get("stats");
  assert.deepEqual(
    stats.variants.map((v) => v.id),
    ["a", "b"],
  );
  assert.deepEqual(stats.variants[0].markers, ["cost-by-model", "effort"]);
  assert.deepEqual(stats.needs, ["history"]);
});

test("adding a scene is adding a file: no list to edit", async (t) => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "readme-media-scenes-"));
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
  const variant = (id) => `{ id: "${id}", viewport: { width: 800, height: 600 }, markers: ["m"], async play() {} }`;
  fs.writeFileSync(path.join(dir, "alpha.mjs"), `export default { name: "alpha", variants: [${variant("a")}, ${variant("b")}] };\n`);
  fs.writeFileSync(path.join(dir, "beta.mjs"), `export default { name: "beta", variants: [${variant("a")}, ${variant("b")}] };\n`);
  fs.writeFileSync(path.join(dir, "_helpers.mjs"), "export const helper = 1;\n");
  const scenes = await loadScenes(dir);
  assert.deepEqual([...scenes.keys()], ["alpha", "beta"]);
  assert.deepEqual(
    selectScenes(scenes, "beta").map((s) => s.name),
    ["beta"],
  );
  assert.throws(() => selectScenes(scenes, "gamma"), /unknown scene/);
});

test("a malformed scene is refused with the reason", () => {
  const variant = { id: "a", viewport: { width: 1, height: 1 }, markers: ["m"], play() {} };
  assert.throws(() => validateScene({ name: "x", variants: [variant] }, "x.mjs"), /exactly two variants/);
  assert.throws(() => validateScene({ name: "x", variants: [variant, variant] }, "x.mjs"), /declared twice/);
  assert.throws(() => validateScene({ name: "x", variants: [variant, { ...variant, id: "b", markers: [] }] }, "x.mjs"), /markers/);
  assert.throws(() => validateScene({ name: "x", variants: [variant, { ...variant, id: "b" }] }, "y.mjs"), /file name must be x\.mjs/);
});
