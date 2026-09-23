// The target pipelines (ADR-0074 §6): installed as is, the run snapshot derived
// from them, and the export / import round trip with the user's library —
// always a throwaway HOME here, never ~/.pdo.

import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { after, test } from "node:test";
import { fileURLToPath } from "node:url";
import { DEMO_PIPELINE } from "../lib/demo-pipeline.mjs";
import {
  ExportConflict,
  exportTargets,
  importTargets,
  installTargets,
  parseYaml,
  readTarget,
  runSnapshot,
  TARGETS,
  TARGETS_DIR,
  targetPipeline,
  withName,
  withNodeFlags,
} from "../lib/targets.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const script = path.join(here, "..", "readme-media.mjs");
const tmpRoot = fs.mkdtempSync(path.join(os.tmpdir(), "readme-media-targets-"));
after(() => fs.rmSync(tmpRoot, { recursive: true, force: true }));

const tmp = (prefix) => fs.mkdtempSync(path.join(tmpRoot, prefix));

/** A copy of the versioned targets, so a test may write into it. */
function targetsCopy() {
  const dir = tmp("targets-");
  fs.cpSync(TARGETS_DIR, dir, { recursive: true });
  return dir;
}

/** Every file of a directory, relative path → content. */
function snapshotDir(dir) {
  const out = {};
  for (const entry of fs.readdirSync(dir, { recursive: true })) {
    const file = path.join(dir, entry);
    if (fs.statSync(file).isFile()) out[entry] = fs.readFileSync(file, "utf8");
  }
  return out;
}

/** Only the `name:` line differs between two YAML texts. */
function assertSameButName(installed, original, name) {
  const a = installed.split("\n");
  const b = original.split("\n");
  assert.equal(a.length, b.length);
  const diff = a.map((line, i) => [line, b[i]]).filter(([x, y]) => x !== y);
  assert.ok(diff.length <= 1, `only name: may differ, got ${JSON.stringify(diff)}`);
  if (diff.length === 1) assert.match(diff[0][1], /^name: /);
  assert.equal(parseYaml(installed).name, name);
}

test("three targets: implement-review built and complete, prod-check; the fixture holds nothing else", () => {
  assert.deepEqual(
    TARGETS.map((t) => [t.id, t.name, t.library]),
    [
      ["implement-review", "implement-review", "readme-implement-review"],
      ["implement-review.pipelines", "implement-review", "readme-pipelines"],
      ["prod-check", "prod-check", "prod-check"],
    ],
  );
  assert.ok(!fs.existsSync(path.join(TARGETS_DIR, "..", "pipelines")), "no hand-written demo pipeline left beside the targets");
  const complete = targetPipeline("implement-review");
  assert.deepEqual(complete.loops.map((l) => [l.kind, l.max_iter, l.members]), [["bounded", 5, ["implementer", "reviewer"]]]);
  assert.deepEqual(
    targetPipeline("implement-review.pipelines").nodes.map((n) => n.id),
    ["start", "end", "implementer"],
  );
});

test("installation: each target byte for byte, only name: set to the demo name, with its prompts", () => {
  const library = tmp("library-");
  const installed = installTargets(library);
  assert.deepEqual(
    installed.map((i) => i.name),
    ["implement-review", "prod-check"],
    "the built state shares its name with the complete one: a scene swaps it in",
  );
  for (const { id, name } of installed) {
    const { target, yaml, prompts } = readTarget(id);
    assert.equal(target.name, name);
    assert.equal(fs.readFileSync(path.join(library, `${name}.yaml`), "utf8"), withName(yaml, name));
    assertSameButName(fs.readFileSync(path.join(library, `${name}.yaml`), "utf8"), yaml, name);
    for (const [node, content] of Object.entries(prompts)) {
      assert.equal(fs.readFileSync(path.join(library, `${name}.prompts`, `${node}.md`), "utf8"), content);
    }
  }
  assert.deepEqual(fs.readdirSync(path.join(library, "implement-review.prompts")).sort(), ["implementer.md", "reviewer.md"]);
  assert.deepEqual(fs.readdirSync(path.join(library, "prod-check.prompts")).sort(), ["RlRyUvAz.md", "XxIN3yX6.md", "wHpeFwT0.md"]);
});

test("withName touches the top-level name only, and refuses a pipeline without one", () => {
  const yaml = readTarget("implement-review").yaml;
  assertSameButName(withName(yaml, "readme-implement-review"), yaml, "readme-implement-review");
  assert.equal(withName(withName(yaml, "x"), "implement-review"), yaml);
  assert.throws(() => withName("version: '1.0'\n", "x"), /name:/);
});

test("the run snapshot is derived from the complete target: the daemon's node_defs, and the #840 edges as drawn", () => {
  const target = targetPipeline("implement-review");
  assert.deepEqual(DEMO_PIPELINE, runSnapshot(target, { id: "implement-review" }));
  assert.equal(DEMO_PIPELINE.id, "implement-review");
  const def = (id) => DEMO_PIPELINE.nodeDefs.find((n) => n.id === id);
  for (const node of target.nodes) {
    assert.deepEqual([def(node.id).view_x, def(node.id).view_y], [node.view.x, node.view.y], `${node.id}: the drawn position`);
    assert.equal(def(node.id).node_type, node.type);
  }
  assert.deepEqual(def("reviewer").outputs, [
    { name: "review", side: "right" },
    { name: "screenshots", side: "right" },
  ]);
  assert.equal(DEMO_PIPELINE.edges.length, target.edges.length);
  const loop = DEMO_PIPELINE.edges.find((e) => e.source_node === "reviewer" && e.target_node === "implementer");
  assert.deepEqual(loop.source_ports, ["review", "screenshots"]);
  assert.equal(loop.source_port, "review");
  assert.deepEqual(loop.when, { "review.verdict": { eq: "fail" } });
  const drawn = target.edges.find((e) => e.target.node === "implementer" && e.source.node === "reviewer");
  for (const key of ["waypoints", "source_anchor", "target_anchor", "output_label_pos", "condition_label_pos", "mode", "target_side"]) {
    assert.deepEqual(loop[key], drawn[key], key);
  }
  const exit = DEMO_PIPELINE.edges.find((e) => e.target_node === "end");
  assert.deepEqual(exit.when, { verdict: { eq: "pass" } });
  assert.deepEqual(DEMO_PIPELINE.loops.map((l) => l.max_iter), [5]);
});

test("export then import without a retouch: no difference in the fixture", () => {
  const targetsDir = targetsCopy();
  const before = snapshotDir(targetsDir);
  const libraryDir = path.join(tmp("home-"), ".pdo", "pipelines");
  const exported = exportTargets({ libraryDir, targetsDir });
  assert.deepEqual(exported.written, ["readme-implement-review", "readme-pipelines", "prod-check"]);
  assert.match(fs.readFileSync(path.join(libraryDir, "readme-implement-review.yaml"), "utf8"), /^name: readme-implement-review$/m);
  assert.deepEqual(fs.readdirSync(path.join(libraryDir, "readme-pipelines.prompts")), ["implementer.md"], "the built state: its own nodes' prompts");
  const imported = importTargets({ libraryDir, targetsDir });
  assert.deepEqual(imported.updated, []);
  assert.deepEqual(imported.warnings, []);
  assert.deepEqual(snapshotDir(targetsDir), before);
  // And again: an export over an identical library writes nothing.
  assert.deepEqual(exportTargets({ libraryDir, targetsDir }).written, []);
});

test("import brings a retouch back under the demo name, prompts included", () => {
  const targetsDir = targetsCopy();
  const libraryDir = tmp("library-");
  exportTargets({ libraryDir, targetsDir });
  const file = path.join(libraryDir, "readme-implement-review.yaml");
  fs.writeFileSync(file, fs.readFileSync(file, "utf8").replace("view: { x: 197, y: 208 }", "view: { x: 197, y: 220 }"));
  fs.writeFileSync(path.join(libraryDir, "readme-implement-review.prompts", "reviewer.md"), "Review it.\n");
  fs.writeFileSync(path.join(libraryDir, "readme-pipelines.prompts", "implementer.md"), "Another prompt.\n");
  const { updated, warnings } = importTargets({ libraryDir, targetsDir });
  assert.deepEqual(updated, ["implement-review"]);
  const yaml = fs.readFileSync(path.join(targetsDir, "implement-review.yaml"), "utf8");
  assert.match(yaml, /^name: implement-review$/m);
  assert.match(yaml, /view: \{ x: 197, y: 220 \}/);
  assert.equal(fs.readFileSync(path.join(targetsDir, "implement-review.prompts", "reviewer.md"), "utf8"), "Review it.\n");
  assert.equal(warnings.length, 1);
  assert.match(warnings[0], /readme-pipelines: the prompt of `implementer`/);
});

test("export refuses to overwrite a readme-* modified in the library, and writes nothing", () => {
  const targetsDir = targetsCopy();
  const libraryDir = tmp("library-");
  exportTargets({ libraryDir, targetsDir });
  const edited = path.join(libraryDir, "readme-pipelines.yaml");
  fs.appendFileSync(edited, "\n");
  fs.rmSync(path.join(libraryDir, "prod-check.yaml"));
  const before = snapshotDir(libraryDir);
  assert.throws(() => exportTargets({ libraryDir, targetsDir }), (error) => error instanceof ExportConflict && /readme-pipelines/.test(error.message));
  assert.deepEqual(snapshotDir(libraryDir), before, "not even the missing prod-check was written");
  // A modified prompt counts as a modification too.
  fs.writeFileSync(edited, withName(readTarget("implement-review.pipelines", { targetsDir }).yaml, "readme-pipelines"));
  fs.writeFileSync(path.join(libraryDir, "readme-implement-review.prompts", "implementer.md"), "Edited.\n");
  assert.throws(() => exportTargets({ libraryDir, targetsDir }), /readme-implement-review/);
});

test("import refuses when a readme-* pipeline is missing from the library", () => {
  const targetsDir = targetsCopy();
  const before = snapshotDir(targetsDir);
  assert.throws(() => importTargets({ libraryDir: tmp("empty-"), targetsDir }), /readme-media-export/);
  assert.deepEqual(snapshotDir(targetsDir), before);
});

test("`make readme-media-export` / `-import` on a throwaway HOME: round trip, then a refused overwrite", () => {
  const home = tmp("home-");
  const run = (command) =>
    spawnSync(process.execPath, ["--disable-warning=ExperimentalWarning", script, command], {
      env: { ...process.env, HOME: home, USERPROFILE: home },
      encoding: "utf8",
    });
  const status = () => execFileSync("git", ["status", "--porcelain", "--", TARGETS_DIR], { encoding: "utf8" });
  const fixtureBefore = status();
  const exported = run("export");
  assert.equal(exported.status, 0, exported.stderr);
  const library = path.join(home, ".pdo", "pipelines");
  assert.deepEqual(fs.readdirSync(library).filter((f) => f.endsWith(".yaml")).sort(), ["prod-check.yaml", "readme-implement-review.yaml", "readme-pipelines.yaml"]);
  const imported = run("import");
  assert.equal(imported.status, 0, imported.stderr);
  assert.match(imported.stdout, /no change/);
  assert.equal(status(), fixtureBefore, "the fixture is untouched by the round trip");
  fs.appendFileSync(path.join(library, "readme-implement-review.yaml"), "\n");
  const refused = run("export");
  assert.equal(refused.status, 1);
  assert.match(refused.stderr, /refusing to overwrite readme-implement-review/);
});

test("withNodeFlags turns flags on for one node, every other byte kept", () => {
  const yaml = readTarget("implement-review").yaml;
  const flagged = withNodeFlags(yaml, "implementer", { interactive: true, orchestrator: true });
  const added = flagged.split("\n").filter((line) => !yaml.split("\n").includes(line));
  assert.deepEqual(added, ["    interactive: true", "    orchestrator: true"]);
  assert.equal(flagged.replace("    interactive: true\n    orchestrator: true\n", ""), yaml);
  const nodes = parseYaml(flagged).nodes;
  assert.equal(nodes.find((n) => n.id === "implementer").orchestrator, true);
  assert.equal(nodes.find((n) => n.id === "reviewer").orchestrator, undefined);
  assert.throws(() => withNodeFlags(yaml, "nobody", { orchestrator: true }), /no node `nobody`/);
  assert.throws(() => withNodeFlags(readTarget("prod-check").yaml, "wHpeFwT0", { orchestrator: true }), /already sets `orchestrator`/);
});
