// The version lines a release rewrites: the workspace's, the extension API's
// (its own manifest and the workspace's dependency on it) and the front's
// shared module, which a daemon test keeps equal to the API. `Cargo.lock`
// follows through cargo.

export const VERSION_FILES = [
  "Cargo.toml",
  "crates/pdo-extension-api/Cargo.toml",
  "frontend/src/shared/extensions/uiModuleVersion.ts",
];

const V = "(\\d+\\.\\d+\\.\\d+)";

// [file, the line, what to write]. The first `version = ` of the workspace
// manifest is `[workspace.package]`'s, the first of the API's its `[package]`.
const LINES = [
  ["Cargo.toml", new RegExp(`^(\\[workspace\\.package\\][^[]*?\\nversion = ")${V}(")`, "m"), (v) => v.workspace],
  ["Cargo.toml", new RegExp(`^(pdo-extension-api = \\{.*version = ")${V}(".*)$`, "m"), (v) => v.api],
  ["crates/pdo-extension-api/Cargo.toml", new RegExp(`^(\\[package\\][^[]*?\\nversion = ")${V}(")`, "m"), (v) => v.api],
  ["frontend/src/shared/extensions/uiModuleVersion.ts", new RegExp(`^(export const UI_MODULE_VERSION = ")${V}(";)$`, "m"), (v) => v.api],
  ["frontend/src/shared/extensions/uiModuleVersion.ts", /^(export const UI_MODULE_MAJOR = )(\d+)(;)$/m, (v) => v.api.split(".")[0]],
];

function find(files, [file, pattern]) {
  const match = pattern.exec(files[file] ?? "");
  if (!match) throw new Error(`${file}: no line matching ${pattern.source}`);
  return match;
}

/** The current versions: `{ workspace, api }`. */
export function readVersions(files) {
  return { workspace: find(files, LINES[0])[2], api: find(files, LINES[2])[2] };
}

/** `files` (path -> text) with every version line set; all lines checked first. */
export function writeVersions(files, versions) {
  for (const line of LINES) find(files, line);
  const out = { ...files };
  for (const [file, pattern, value] of LINES) {
    out[file] = out[file].replace(pattern, (_, before, _old, after) => `${before}${value(versions)}${after}`);
  }
  return out;
}
