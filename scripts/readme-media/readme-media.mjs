#!/usr/bin/env node
// `make readme-media [SCENE=…]` and `make readme-media-publish [SCENE=…]`.
//
//   record   — for each scene: a fresh demo instance (ADR-0074), its mocked
//              history if the scene needs it, the auth of the harnesses it plays
//              live; each of its two variants is filmed, cut and framed into
//              .readme-media/<scene>/<variant>.gif + .jpg, and its manifest
//              entry is written as it lands. The instance is torn down after each scene, and on any
//              exit (failure and Ctrl+C included).
//   publish  — copy the variant the selection file names, per scene, into
//              docs/assets/readme/<scene>.gif + .jpg. Records nothing.
//   export   — copy the target pipelines (fixture/targets/) into your library
//              (~/.pdo/pipelines) as readme-*, to redraw them in the editor.
//              Refuses to overwrite a readme-* pipeline modified since.
//   import   — the reverse: your readme-* pipelines back into the fixture,
//              `name:` set back to the demo name.
//
// See scripts/readme-media/README.md.

import fs from "node:fs";
import { createRequire } from "node:module";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { DemoInstance } from "./lib/demo-instance.mjs";
import { landVariant, variantEntry } from "./lib/manifest.mjs";
import { chromeLayout, planCuts, renderChrome, renderVariant, TARGET_MS } from "./lib/montage.mjs";
import { Recorder } from "./lib/recorder.mjs";
import { loadScenes, selectScenes } from "./lib/scenes.mjs";
import { seedHistory } from "./lib/seed.mjs";
import { publish, readSelection } from "./lib/selection.mjs";
import { exportTargets, importTargets, TARGETS_DIR } from "./lib/targets.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, "..", "..");
export const PATHS = {
  scenes: path.join(here, "scenes"),
  selection: path.join(here, "selection.txt"),
  review: path.join(repoRoot, ".readme-media"),
  assets: path.join(repoRoot, "docs", "assets", "readme"),
};
const DEFAULT_GIF_WIDTH = 960;

async function record(filter) {
  const scenes = selectScenes(await loadScenes(PATHS.scenes), filter);
  const require = createRequire(path.join(repoRoot, "frontend", "package.json"));
  const { chromium } = require("@playwright/test");
  fs.mkdirSync(PATHS.review, { recursive: true });
  const manifestFile = path.join(PATHS.review, "manifest.json");
  const failures = [];

  const browser = await chromium.launch();
  try {
    for (const scene of scenes) {
      console.log(`\n== scene ${scene.name}`);
      const instance = new DemoInstance({ repoRoot, liveHarnesses: scene.live ?? [], keep: process.env.KEEP_DEMO === "1" });
      try {
        await instance.start();
        if ((scene.needs ?? []).includes("history")) {
          const seeded = await seedHistory(instance);
          console.log(`mocked history: ${seeded.runs} runs, ${seeded.fires} trigger fires`);
        }
        await scene.setup?.(instance);
        const variants = process.env.VARIANT ? scene.variants.filter((v) => v.id === process.env.VARIANT) : scene.variants;
        for (const variant of variants) {
          try {
            await recordVariant({ browser, instance, scene, variant, manifestFile });
          } catch (error) {
            failures.push(`${scene.name}/${variant.id}: ${error.message}`);
            console.error(`variant ${scene.name}/${variant.id} failed:\n${error.stack}`);
          }
        }
      } catch (error) {
        failures.push(`${scene.name}: ${error.message}`);
        console.error(`scene ${scene.name} failed:\n${error.stack}`);
      } finally {
        // Stop the scene's agents and daemon now, not at the end of the run.
        instance.teardown();
      }
    }
  } finally {
    await browser.close();
  }
  console.log(`\nreview folder: ${PATHS.review} (manifest.json)`);
  if (failures.length > 0) {
    console.error(`\n${failures.length} failure(s):\n- ${failures.join("\n- ")}`);
    process.exitCode = 1;
  }
}

async function recordVariant({ browser, instance, scene, variant, manifestFile }) {
  const gifWidth = variant.gifWidth ?? DEFAULT_GIF_WIDTH;
  const crop = variant.crop ?? { x: 0, y: 0, ...variant.viewport };
  const workDir = path.join(PATHS.review, ".work", scene.name, variant.id);
  fs.rmSync(workDir, { recursive: true, force: true });
  fs.mkdirSync(workDir, { recursive: true });

  console.log(`-- ${scene.name}/${variant.id}: ${variant.label ?? ""}`);
  const recorder = new Recorder({ browser, instance, variant: { ...variant, crop }, videoDir: workDir, gifWidth });
  await recorder.open();
  let timeline;
  try {
    await variant.play(recorder);
  } finally {
    timeline = await recorder.close();
  }

  const plan = planCuts(timeline);
  fs.writeFileSync(path.join(workDir, "timeline.json"), `${JSON.stringify({ timeline, plan }, null, 2)}\n`);
  const layout = chromeLayout({ gifWidth, crop });
  const chromePng = path.join(workDir, "chrome.png");
  await renderChrome(browser, layout, chromePng);
  // Encoded in the work folder; lands in the review folder only once complete.
  const encodedGif = path.join(workDir, "out.gif");
  const encodedPoster = path.join(workDir, "out.jpg");
  const facts = await renderVariant({ video: timeline.video, plan, crop, layout, chromePng, workDir, gifFile: encodedGif, posterFile: encodedPoster });
  const gif = path.join(PATHS.review, scene.name, `${variant.id}.gif`);
  const poster = path.join(PATHS.review, scene.name, `${variant.id}.jpg`);
  const entry = variantEntry({
    variant,
    facts,
    plan,
    gif: path.relative(PATHS.review, gif),
    poster: path.relative(PATHS.review, poster),
    posterBytes: fs.statSync(encodedPoster).size,
    target: TARGET_MS,
  });
  landVariant({
    file: manifestFile,
    sceneName: scene.name,
    title: scene.title ?? scene.name,
    entry,
    moves: [
      [encodedGif, gif],
      [encodedPoster, poster],
    ],
  });
  console.log(`   ${entry.duration_s}s · ${entry.width}×${entry.height} · ${(entry.bytes / 1e6).toFixed(1)} MB · markers ${entry.markers.map((m) => `${m.name}@${m.t_s}s`).join(", ")}`);
  for (const warning of entry.warnings) console.warn(`   WARNING: ${warning}`);
  return entry;
}

async function publishSelected(filter) {
  const scenes = await loadScenes(PATHS.scenes);
  const only = filter ? selectScenes(scenes, filter).map((s) => s.name) : null;
  publish({ selection: readSelection(PATHS.selection), scenes, reviewDir: PATHS.review, assetsDir: PATHS.assets, only });
}

/** Your library: `~/.pdo/pipelines` of the calling HOME (a test passes its own HOME). */
function userLibrary() {
  return path.join(os.homedir(), ".pdo", "pipelines");
}

function exportToLibrary() {
  const libraryDir = userLibrary();
  const { written, unchanged } = exportTargets({ libraryDir });
  for (const name of written) console.log(`exported ${name} → ${path.join(libraryDir, `${name}.yaml`)}`);
  for (const name of unchanged) console.log(`${name}: already up to date in ${libraryDir}`);
  console.log("Redraw them in the PDO editor, then `make readme-media-import`.");
}

function importFromLibrary() {
  const { updated, unchanged, warnings } = importTargets({ libraryDir: userLibrary() });
  for (const id of updated) console.log(`imported ${id} → ${path.relative(repoRoot, TARGETS_DIR)}/`);
  if (updated.length === 0) console.log(`no change: the fixture already matches your library (${unchanged.join(", ")})`);
  for (const warning of warnings) console.warn(`WARNING: ${warning}`);
}

const [command = "record"] = process.argv.slice(2);
const filter = process.env.SCENE || null;
try {
  if (command === "record") await record(filter);
  else if (command === "publish") await publishSelected(filter);
  else if (command === "export") exportToLibrary();
  else if (command === "import") importFromLibrary();
  else throw new Error(`unknown command "${command}" (record | publish | export | import)`);
} catch (error) {
  console.error(error.message);
  process.exitCode = 1;
}
