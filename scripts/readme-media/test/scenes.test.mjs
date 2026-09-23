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

test("the canvas scenes (#858): hero, pipelines and routing, two variants each and their markers", async () => {
  const scenes = await loadScenes(path.join(here, "..", "scenes"));
  const markers = (name) => scenes.get(name).variants.map((v) => [v.id, v.markers]);
  assert.deepEqual(markers("hero"), [
    ["a", ["run-started", "zoom", "terminal-active", "verdict"]],
    ["b", ["run-started", "terminal-active", "output-ready"]],
  ]);
  assert.deepEqual(markers("pipelines"), [
    ["a", ["node-added", "edge-dropped"]],
    ["b", ["node-added", "edge-dropped"]],
  ]);
  assert.deepEqual(markers("routing"), [
    ["a", ["edge-dropped", "condition-saved"]],
    ["b", ["edge-dropped", "condition-saved"]],
  ]);
  // Only the hero plays agents live (and needs their auth); the canvas rows do not.
  assert.deepEqual(scenes.get("hero").live, ["claude"]);
  assert.deepEqual(scenes.get("pipelines").live, []);
  assert.deepEqual(scenes.get("routing").live, []);
  // The published hero is design variant B: full window 1280×760, inspector at 43 %, no crop.
  const heroB = scenes.get("hero").variants.find((v) => v.id === "b");
  assert.deepEqual(heroB.viewport, { width: 1280, height: 760 });
  assert.equal(heroB.crop, undefined);
  assert.deepEqual(heroB.localStorage["pdo.layout.run"], { left: 15, center: 42, right: 43 });
});

test("the artifact scenes (#859): outputs, review and orchestration, two variants each and their markers", async () => {
  const scenes = await loadScenes(path.join(here, "..", "scenes"));
  const markers = (name) => scenes.get(name).variants.map((v) => [v.id, v.markers]);
  assert.deepEqual(markers("outputs"), [
    ["a", ["ports-declared", "image-opened", "mermaid-rendered"]],
    ["b", ["ports-declared", "image-opened", "mermaid-rendered"]],
  ]);
  assert.deepEqual(markers("review"), [
    ["a", ["comment-posted", "sent-to-manager", "answer-received"]],
    ["b", ["comment-posted", "sent-to-manager", "answer-received"]],
  ]);
  assert.deepEqual(markers("orchestration"), [
    ["a", ["child-created", "counters"]],
    ["b", ["child-created", "counters"]],
  ]);
  // All three are played by real agents: the claude auth is staged for them.
  for (const name of ["outputs", "review", "orchestration"]) assert.deepEqual(scenes.get(name).live, ["claude"], name);
  // A row GIF stays readable at ~470 px: its crop is never wider than the window.
  for (const name of ["outputs", "review", "orchestration"]) {
    for (const v of scenes.get(name).variants) {
      const crop = v.crop ?? { x: 0, y: 0, ...v.viewport };
      assert.ok(crop.x + crop.width <= v.viewport.width && crop.y + crop.height <= v.viewport.height, `${name}/${v.id} crop inside the window`);
    }
  }
});

test("orchestration relaunches the demo pipeline: its target, Orchestrator on for implementer, nothing else", async () => {
  const { orchestratorYaml, ORCHESTRATION_TASK, PARTS } = await import("../scenes/orchestration.mjs");
  const target = fs.readFileSync(path.join(here, "..", "fixture", "targets", "implement-review.yaml"), "utf8");
  const yaml = orchestratorYaml();
  const added = yaml.split("\n").filter((line) => !target.split("\n").includes(line));
  assert.deepEqual(added, ["    orchestrator: true"]);
  assert.match(yaml, /- id: implementer\n {4}name: implementer\n {4}type: agent\n {4}orchestrator: true\n/);
  assert.equal(yaml.match(/orchestrator: true/g).length, 1, "the reviewer stays a plain node");
  // Every child is a run of the same pipeline, told not to orchestrate in turn.
  assert.equal(PARTS.length, 2);
  for (const part of PARTS) {
    assert.ok(ORCHESTRATION_TASK.input.includes(`pdo run create implement-review --name "${part.name}"`), part.name);
    assert.match(part.input, /do not create child runs/);
  }
  assert.doesNotMatch(ORCHESTRATION_TASK.input, /pdo run create (?!implement-review)/, "every child runs implement-review");
});

test("a live scene only films a run that ends completed on a final `pass` (the reviewer's last lap)", async () => {
  const { finalVerdict, waitRunPassed } = await import("../scenes/_live.mjs");
  /** A demo instance's API, reduced to a run and its reviewer's `review` outputs per lap. */
  const fakeInstance = ({ status, verdicts }) => ({
    async api(method, route) {
      if (route === "/runs/r1") return { status, nodes: { reviewer: { status: "completed", iter: verdicts.length } } };
      const iter = Number(route.match(/iter=(\d+)$/)?.[1]);
      return { outputs: [{ port: "review", files: [{ path: "output.md", frontmatter: { verdict: verdicts[iter - 1] } }] }] };
    },
  });
  // A lap sent back, then a pass: the verdict read is the last lap's.
  assert.deepEqual(await finalVerdict(fakeInstance({ status: "completed", verdicts: ["fail", "pass"] }), "r1"), { verdict: "pass", iter: 2 });
  assert.equal((await waitRunPassed(fakeInstance({ status: "completed", verdicts: ["fail", "pass"] }), "r1")).iter, 2);
  await assert.rejects(waitRunPassed(fakeInstance({ status: "completed", verdicts: ["pass", "fail"] }), "r1"), /final verdict fail/);
  await assert.rejects(waitRunPassed(fakeInstance({ status: "failed", verdicts: ["fail", "fail", "fail", "fail", "fail"] }), "r1"), /ended failed/);
});
