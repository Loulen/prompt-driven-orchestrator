// The media config (ADR-0074, amended by #36): what a consumer of the tool
// declares — the core's own config first, then any other repo's, called from a
// checkout of the core (`make readme-media CONFIG=<file>`). One config per
// consumer repo; the tool itself (demo instance, recorder, montage, selection)
// is the same for all of them.
//
// A config is an ES module whose default export is a plain object:
//
//     export default {
//       binary: "../../target/debug/pdo",   // the PDO the demo instance starts
//       scenes: "./scenes",                  // the scene files (lib/scenes.mjs)
//       fixture: {                           // optional, both keys optional
//         repo: "./fixture/shop-app",        //   copied as a fresh git repo to ~/code/<its name>
//         pipelines: "./fixture/pipelines",  //   a library folder copied as is, or a function(libraryDir)
//       },
//       selection: "./selection.txt",        // which variant of each scene is published
//       review: "../../.readme-media",       // where the two variants of each scene land
//       destination: "../../docs/assets/readme", // where the selected GIF + poster are published
//       seed: async (instance, scene) => {}, // optional: the demo instance's bootstrap
//       playwright: "../../frontend",        // optional: where @playwright/test resolves from
//     };
//
// Every relative path is resolved from the config file's own folder, never from
// the caller's cwd. A missing or misspelt field, a path that does not exist or a
// binary that cannot run fails with a message naming the field and the path.

import fs from "node:fs";
import { createRequire } from "node:module";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));

/** The tool's folder (scripts/readme-media/ of the core checkout). */
export const TOOL_DIR = path.resolve(here, "..");
/** The core's own config: what `make readme-media` uses without CONFIG. */
export const CORE_CONFIG = path.join(TOOL_DIR, "readme-media.config.mjs");
/** Where Playwright resolves from when a config names no `playwright`. */
const CORE_FRONTEND = path.resolve(TOOL_DIR, "..", "..", "frontend");

/** What each field is for: the error of a missing one says it. */
const REQUIRED = {
  binary: "the PDO binary the demo instance starts",
  scenes: "the folder of the scene files",
  selection: "the selection file (`<scene> <variant>` per line)",
  review: "the review folder, where the two variants of each scene land",
  destination: "the folder the selected GIF and poster of each scene are published into",
};
const OPTIONAL = ["fixture", "seed", "playwright"];
const FIXTURE_KEYS = ["repo", "pipelines"];

export class ConfigError extends Error {}

/** Load, check and resolve the config at `file`. */
export async function loadConfig(file = CORE_CONFIG) {
  const absolute = path.resolve(file);
  if (!fs.existsSync(absolute)) throw new ConfigError(`readme-media config not found: ${absolute}`);
  let mod;
  try {
    mod = await import(pathToFileURL(absolute).href);
  } catch (error) {
    throw new ConfigError(`readme-media config ${absolute}: cannot be loaded: ${error.message}`);
  }
  return resolveConfig(mod.default, absolute);
}

/** Check a config object and resolve its paths against `file`'s folder. Pure
 *  but for the existence checks of the paths it must read. */
export function resolveConfig(raw, file) {
  const fail = (why) => {
    throw new ConfigError(`readme-media config ${file}: ${why}`);
  };
  if (!raw || typeof raw !== "object" || Array.isArray(raw)) fail("must `export default` a config object");
  const unknown = Object.keys(raw).filter((key) => !(key in REQUIRED) && !OPTIONAL.includes(key));
  if (unknown.length > 0) fail(`unknown field(s) ${unknown.map((k) => `\`${k}\``).join(", ")} (known: ${[...Object.keys(REQUIRED), ...OPTIONAL].join(", ")})`);
  for (const [key, why] of Object.entries(REQUIRED)) {
    if (raw[key] === undefined || raw[key] === null || raw[key] === "") fail(`missing \`${key}\`: ${why}`);
    if (typeof raw[key] !== "string") fail(`\`${key}\` must be a path (a string)`);
  }
  const dir = path.dirname(file);
  const at = (value) => path.resolve(dir, value);
  const mustExist = (key, value, kind) => {
    const resolved = at(value);
    let stat = null;
    try {
      stat = fs.statSync(resolved);
    } catch {
      fail(`\`${key}\` → ${resolved}: no such ${kind}`);
    }
    if (kind === "folder" && !stat.isDirectory()) fail(`\`${key}\` → ${resolved}: not a folder`);
    if (kind === "file" && !stat.isFile()) fail(`\`${key}\` → ${resolved}: not a file`);
    return resolved;
  };

  const config = {
    file,
    binary: at(raw.binary),
    scenes: mustExist("scenes", raw.scenes, "folder"),
    selection: mustExist("selection", raw.selection, "file"),
    review: at(raw.review),
    destination: at(raw.destination),
    fixture: { repo: null, pipelines: null },
    seed: null,
    playwright: CORE_FRONTEND,
  };

  if (raw.fixture !== undefined) {
    if (!raw.fixture || typeof raw.fixture !== "object" || Array.isArray(raw.fixture)) fail("`fixture` must be an object (`repo`, `pipelines`)");
    const extra = Object.keys(raw.fixture).filter((key) => !FIXTURE_KEYS.includes(key));
    if (extra.length > 0) fail(`unknown field(s) ${extra.map((k) => `\`fixture.${k}\``).join(", ")} (known: fixture.repo, fixture.pipelines)`);
    const { repo, pipelines } = raw.fixture;
    if (repo !== undefined) {
      if (typeof repo !== "string") fail("`fixture.repo` must be a path (a string)");
      config.fixture.repo = mustExist("fixture.repo", repo, "folder");
    }
    if (pipelines !== undefined) {
      if (typeof pipelines === "function") config.fixture.pipelines = pipelines;
      else if (typeof pipelines === "string") config.fixture.pipelines = mustExist("fixture.pipelines", pipelines, "folder");
      else fail("`fixture.pipelines` must be a folder (a string) or a function(libraryDir)");
    }
  }
  if (raw.seed !== undefined) {
    if (typeof raw.seed !== "function") fail("`seed` must be a function(instance, scene)");
    config.seed = raw.seed;
  }
  if (raw.playwright !== undefined) {
    if (typeof raw.playwright !== "string") fail("`playwright` must be a path (a string)");
    config.playwright = mustExist("playwright", raw.playwright, "folder");
  }
  return config;
}

/** What a DemoInstance takes from a config. */
export function demoOptions(config) {
  return { binary: config.binary, fixtureRepo: config.fixture.repo, pipelines: config.fixture.pipelines };
}

/** What `record` needs on top of a valid config: a binary that runs. */
export function assertRecordable(config) {
  const fail = (why) => {
    throw new ConfigError(`readme-media config ${config.file}: ${why}`);
  };
  let stat;
  try {
    stat = fs.statSync(config.binary);
  } catch {
    fail(`\`binary\` → ${config.binary}: no such file (build it first)`);
  }
  if (!stat.isFile()) fail(`\`binary\` → ${config.binary}: not a file`);
  try {
    fs.accessSync(config.binary, fs.constants.X_OK);
  } catch {
    fail(`\`binary\` → ${config.binary}: not executable`);
  }
}

/** `@playwright/test`, resolved from the config's `playwright` folder. */
export function resolvePlaywright(config) {
  try {
    return createRequire(path.join(config.playwright, "package.json"))("@playwright/test");
  } catch (error) {
    throw new ConfigError(
      `readme-media config ${config.file}: \`playwright\` → ${config.playwright}: @playwright/test does not resolve from there (${error.code ?? error.message}) — \`pnpm install\` in that folder`,
    );
  }
}
