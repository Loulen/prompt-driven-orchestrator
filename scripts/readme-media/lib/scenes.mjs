// Scene discovery: every `scenes/<name>.mjs` is a scene, found by listing the
// folder — there is no central list to edit, so two scenes added in parallel
// never touch the same file. See scripts/readme-media/README.md for the shape
// a scene file exports.

import fs from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

export async function loadScenes(scenesDir) {
  const scenes = new Map();
  const files = fs
    .readdirSync(scenesDir)
    .filter((name) => name.endsWith(".mjs") && !name.startsWith("_"))
    .sort();
  for (const file of files) {
    const mod = await import(pathToFileURL(path.join(scenesDir, file)).href);
    const scene = mod.default;
    validateScene(scene, file);
    if (scenes.has(scene.name)) throw new Error(`scene "${scene.name}" is declared twice (${file})`);
    scenes.set(scene.name, scene);
  }
  return scenes;
}

export function validateScene(scene, file = scene?.name) {
  const fail = (why) => {
    throw new Error(`scene file ${file}: ${why}`);
  };
  if (!scene || typeof scene !== "object") fail("must `export default` a scene object");
  if (!/^[a-z][a-z0-9-]*$/.test(scene.name ?? "")) fail("`name` must be a lowercase slug");
  if (file && file !== scene.name && file !== `${scene.name}.mjs`) fail(`file name must be ${scene.name}.mjs`);
  if (!Array.isArray(scene.variants) || scene.variants.length !== 2) fail("declares exactly two variants");
  const ids = new Set();
  for (const variant of scene.variants) {
    if (!/^[a-z][a-z0-9-]*$/.test(variant.id ?? "")) fail("each variant needs an `id` slug");
    if (ids.has(variant.id)) fail(`variant "${variant.id}" declared twice`);
    ids.add(variant.id);
    if (typeof variant.play !== "function") fail(`variant "${variant.id}" needs a \`play(ctx)\` function`);
    if (!variant.viewport?.width || !variant.viewport?.height) fail(`variant "${variant.id}" needs a \`viewport\``);
    if (!Array.isArray(variant.markers) || variant.markers.length === 0) fail(`variant "${variant.id}" must list the \`markers\` it sets`);
  }
  for (const key of ["live", "needs"]) {
    if (scene[key] !== undefined && !Array.isArray(scene[key])) fail(`\`${key}\` must be an array`);
  }
}

/** Which scenes a run records: all of them, or the comma-separated `SCENE`. */
export function selectScenes(scenes, filter) {
  if (!filter) return [...scenes.values()];
  const names = filter
    .split(",")
    .map((s) => s.trim())
    .filter(Boolean);
  const unknown = names.filter((name) => !scenes.has(name));
  if (unknown.length > 0) throw new Error(`unknown scene(s): ${unknown.join(", ")} — known: ${[...scenes.keys()].join(", ")}`);
  return names.map((name) => scenes.get(name));
}
