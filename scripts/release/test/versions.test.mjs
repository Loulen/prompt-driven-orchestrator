import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { VERSION_FILES, readVersions, writeVersions } from "../lib/versions.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../..");
const read = () => Object.fromEntries(VERSION_FILES.map((f) => [f, fs.readFileSync(path.join(root, f), "utf8")]));

test("the versions are read from this repository's own files", () => {
  const { workspace, api } = readVersions(read());
  assert.match(workspace, /^\d+\.\d+\.\d+$/);
  assert.match(api, /^0\.\d+\.\d+$/);
});

test("writing bumps the workspace, the API, its dependency and the shared module", () => {
  const before = read();
  const after = writeVersions(before, { workspace: "9.1.0", api: "1.2.3" });
  assert.deepEqual(readVersions(after), { workspace: "9.1.0", api: "1.2.3" });
  assert.match(after["Cargo.toml"], /^pdo-extension-api = \{ path = "crates\/pdo-extension-api", version = "1\.2\.3" \}$/m);
  const ui = after["frontend/src/shared/extensions/uiModuleVersion.ts"];
  assert.match(ui, /^export const UI_MODULE_VERSION = "1\.2\.3";$/m);
  assert.match(ui, /^export const UI_MODULE_MAJOR = 1;$/m);
  // Nothing else moves: only the version lines differ.
  const changed = (file) => before[file].split("\n").filter((line, i) => line !== after[file].split("\n")[i]).length;
  assert.deepEqual(VERSION_FILES.map(changed), [2, 1, 2]);
});

test("a file missing its version line is refused, never written half", () => {
  const files = { ...read(), "frontend/src/shared/extensions/uiModuleVersion.ts": "export {};\n" };
  assert.throws(() => writeVersions(files, { workspace: "9.1.0", api: "1.2.3" }), /UI_MODULE_VERSION/);
});
