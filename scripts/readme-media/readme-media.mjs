#!/usr/bin/env node
// `make readme-media [SCENE=…] [CONFIG=…]` and `make readme-media-publish [SCENE=…] [CONFIG=…]`.
//
//   readme-media.mjs <command> [--config <file>]
//
// Everything a consumer owns comes from its config (lib/config.mjs): the
// binary, the scenes, the fixture, the selection, the review folder, the
// destination and the bootstrap (`seed`). Without `--config`, the core's own
// (readme-media.config.mjs). Paths below are the core's.
//
//   record   — for each scene: a fresh demo instance (ADR-0074), the config's
//              `seed` (the core's: its mocked history if the scene needs it), the
//              auth of the harnesses it plays live; each of its two variants is
//              filmed, cut and framed into .readme-media/<scene>/<variant>.gif +
//              .jpg, and its manifest entry is written as it lands. The instance
//              is torn down after each scene, and on any exit (failure and
//              Ctrl+C included).
//   publish  — copy the variant the selection file names, per scene, into
//              docs/assets/readme/<scene>.gif + .jpg. Records nothing.
//   export   — copy the core's target pipelines (fixture/targets/) into your
//              library (~/.pdo/pipelines) as readme-*, to redraw them in the
//              editor. Refuses to overwrite a readme-* pipeline modified since.
//   import   — the reverse: your readme-* pipelines back into the fixture,
//              `name:` set back to the demo name.
//              export and import are the core's own: they take no config.
//
// See scripts/readme-media/README.md.

import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { assertRecordable, CORE_CONFIG, demoOptions, loadConfig, resolvePlaywright, TOOL_DIR } from "./lib/config.mjs";
import { DemoInstance } from "./lib/demo-instance.mjs";
import { landVariant, variantEntry } from "./lib/manifest.mjs";
import { chromeLayout, planCuts, renderChrome, renderVariant, TARGET_MS } from "./lib/montage.mjs";
import { Recorder } from "./lib/recorder.mjs";
import { assertCleanScreen } from "./lib/screen-guard.mjs";
import { loadScenes, selectScenes } from "./lib/scenes.mjs";
import { publish, readSelection } from "./lib/selection.mjs";
import { exportTargets, importTargets, TARGETS_DIR } from "./lib/targets.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, "..", "..");
const DEFAULT_GIF_WIDTH = 960;
// A scene of another repo imports the tool's helpers from here
// (`${process.env.README_MEDIA_TOOL}/scenes/_no-agent.mjs`, …).
process.env.README_MEDIA_TOOL = TOOL_DIR;

async function record(config, filter) {
  assertRecordable(config);
  const scenes = selectScenes(await loadScenes(config.scenes), filter);
  const { chromium } = resolvePlaywright(config);
  fs.mkdirSync(config.review, { recursive: true });
  const manifestFile = path.join(config.review, "manifest.json");
  const failures = [];

  const browser = await chromium.launch();
  try {
    for (const scene of scenes) {
      console.log(`\n== scene ${scene.name}`);
      const instance = new DemoInstance({
        ...demoOptions(config),
        liveHarnesses: scene.live ?? [],
        keep: process.env.KEEP_DEMO === "1",
      });
      try {
        await instance.start();
        // The first GET /settings pays the host probes (tens of seconds on a busy
        // machine), then the daemon caches them: pay it here, off camera, before
        // a Settings page waits on it (profiles, skills, the trigger's `~` path).
        await instance.api("GET", "/settings");
        // The consumer's bootstrap (the core's: its mocked history).
        await config.seed?.(instance, scene);
        await scene.setup?.(instance);
        const variants = process.env.VARIANT ? scene.variants.filter((v) => v.id === process.env.VARIANT) : scene.variants;
        for (const variant of variants) {
          try {
            await recordVariant({ browser, instance, scene, variant, manifestFile, reviewDir: config.review });
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
  console.log(`\nreview folder: ${config.review} (manifest.json)`);
  if (failures.length > 0) {
    console.error(`\n${failures.length} failure(s):\n- ${failures.join("\n- ")}`);
    process.exitCode = 1;
  }
}

async function recordVariant({ browser, instance, scene, variant, manifestFile, reviewDir }) {
  const gifWidth = variant.gifWidth ?? DEFAULT_GIF_WIDTH;
  const crop = variant.crop ?? { x: 0, y: 0, ...variant.viewport };
  const workDir = path.join(reviewDir, ".work", scene.name, variant.id);
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
  // No demo path (/tmp/pdo-readme-media-*) in what the GIF keeps: the variant fails.
  assertCleanScreen(timeline);
  const layout = chromeLayout({ gifWidth, crop });
  const chromePng = path.join(workDir, "chrome.png");
  await renderChrome(browser, layout, chromePng);
  // Encoded in the work folder; lands in the review folder only once complete.
  const encodedGif = path.join(workDir, "out.gif");
  const encodedPoster = path.join(workDir, "out.jpg");
  const facts = await renderVariant({ video: timeline.video, plan, crop, layout, chromePng, workDir, gifFile: encodedGif, posterFile: encodedPoster });
  const gif = path.join(reviewDir, scene.name, `${variant.id}.gif`);
  const poster = path.join(reviewDir, scene.name, `${variant.id}.jpg`);
  const entry = variantEntry({
    variant,
    facts,
    plan,
    gif: path.relative(reviewDir, gif),
    poster: path.relative(reviewDir, poster),
    posterBytes: fs.statSync(encodedPoster).size,
    target: TARGET_MS,
    sceneWarnings: timeline.warnings,
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

async function publishSelected(config, filter) {
  const scenes = await loadScenes(config.scenes);
  const only = filter ? selectScenes(scenes, filter).map((s) => s.name) : null;
  publish({ selection: readSelection(config.selection), scenes, reviewDir: config.review, assetsDir: config.destination, only });
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

/** `<command> [--config <file>]`; the core's config without `--config`. */
function parseArgs(argv) {
  const positional = [];
  let config = null;
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === "--config") {
      config = argv[++i];
      if (!config) throw new Error("--config needs a file");
    } else if (arg.startsWith("--config=")) {
      config = arg.slice("--config=".length);
      if (!config) throw new Error("--config needs a file");
    } else if (arg.startsWith("-")) {
      throw new Error(`unknown option "${arg}" (--config <file>)`);
    } else positional.push(arg);
  }
  if (positional.length > 1) throw new Error(`one command expected, got ${positional.join(" ")}`);
  return { command: positional[0] ?? "record", config };
}

const filter = process.env.SCENE || null;
try {
  const { command, config: configFile } = parseArgs(process.argv.slice(2));
  if (command === "record") await record(await loadConfig(configFile ?? CORE_CONFIG), filter);
  else if (command === "publish") await publishSelected(await loadConfig(configFile ?? CORE_CONFIG), filter);
  else if (command === "export" || command === "import") {
    // The core's target pipelines (fixture/targets/): another repo's config has none.
    if (configFile && path.resolve(configFile) !== CORE_CONFIG) throw new Error(`\`${command}\` moves the core's target pipelines only: run it without a config`);
    if (command === "export") exportToLibrary();
    else importFromLibrary();
  } else throw new Error(`unknown command "${command}" (record | publish | export | import)`);
} catch (error) {
  console.error(error.message);
  process.exitCode = 1;
}
