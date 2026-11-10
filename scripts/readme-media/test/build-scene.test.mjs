// The scenes that build on the canvas (#882): the start state is the target
// minus what the scene draws, and a gesture that drifts from the target lands in
// the manifest as a warning.

import assert from "node:assert/strict";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { DRIFT_TOLERANCE, cursorPath, drawnRoute, drawnY, driftWarnings, edgeKey, gestureDrift, startState } from "../lib/build-scene.mjs";
import { variantEntry } from "../lib/manifest.mjs";
import { TARGET_MS } from "../lib/montage.mjs";
import { loadScenes, validateScene } from "../lib/scenes.mjs";
import { dumpYaml, parseYaml, targetPipeline } from "../lib/targets.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const scenes = await loadScenes(path.join(here, "..", "scenes"));
const built = targetPipeline("implement-review.pipelines");
const complete = targetPipeline("implement-review");

/** Node ids, edge keys (with their condition) and loop regions of a pipeline. */
function outline(pipeline) {
  return {
    nodes: pipeline.nodes.map((n) => n.id).sort(),
    edges: pipeline.edges.map((e) => `${edgeKey(e)}${e.when ? ` when ${JSON.stringify(e.when)}` : ""}`).sort(),
    loops: (pipeline.loops ?? []).map((l) => `${l.members.join(",")} ↻ ${l.max_iter}`),
  };
}

/** `pipeline` without the fields `drop` names, deep. */
function without(pipeline, drop) {
  return JSON.parse(JSON.stringify(pipeline, (key, value) => (drop.includes(key) ? undefined : value)));
}

test("pipelines starts from its target without implementer and its two edges", () => {
  const { drawn } = scenes.get("pipelines");
  const start = startState(built, drawn);
  assert.deepEqual(outline(start), { nodes: ["end", "start"], edges: [], loops: [] });
  // Start and End stay exactly where the maintainer drew them.
  assert.deepEqual(start.nodes, built.nodes.filter((n) => n.id !== "implementer"));
  assert.deepEqual(outline(built).edges, ["implementer→end", "start→implementer"]);
});

test("routing starts from the complete target without the loop edge, its region and the exit's condition", () => {
  const { drawn } = scenes.get("routing");
  const start = startState(complete, drawn);
  assert.deepEqual(outline(start), {
    nodes: ["end", "implementer", "reviewer", "start"],
    edges: ["implementer→reviewer", "reviewer→end", "start→implementer"],
    loops: [],
  });
  const exit = (p) => p.edges.find((e) => edgeKey(e) === "reviewer→end");
  // The exit keeps its route, anchors and output label; only the condition and its pill go.
  assert.deepEqual(exit(start), without(exit(complete), ["when", "condition_label_pos"]));
  // Everything else is the target's, byte for byte once parsed.
  assert.deepEqual(start.nodes, complete.nodes);
  assert.deepEqual(
    start.edges.filter((e) => edgeKey(e) !== "reviewer→end"),
    complete.edges.filter((e) => !["reviewer→end", "reviewer→implementer"].includes(edgeKey(e))),
  );
});

test("the start state is pure, and refuses what the target does not have", () => {
  const before = JSON.stringify(complete);
  startState(complete, { nodes: ["implementer"], edges: ["reviewer→implementer"], conditions: ["reviewer→end"] });
  assert.equal(JSON.stringify(complete), before);
  assert.throws(() => startState(built, { nodes: ["reviewer"] }), /no node `reviewer`/);
  assert.throws(() => startState(built, { edges: ["end→start"] }), /no edge `end→start`/);
  assert.throws(() => startState(built, { conditions: ["start→implementer"] }), /has no condition/);
  // Removing a node takes its edges, and a region it was part of if its cycle breaks.
  const noImplementer = startState(complete, { nodes: ["implementer"] });
  assert.deepEqual(outline(noImplementer), { nodes: ["end", "reviewer", "start"], edges: ['reviewer→end when {"verdict":{"eq":"pass"}}'], loops: [] });
  // A region whose cycle still closes stays.
  assert.deepEqual(outline(startState(complete, { conditions: ["reviewer→end"] })).loops, ["implementer,reviewer ↻ 5"]);
});

test("a gesture that lands on the target has no drift, whatever the editor's ids and hidden fields", () => {
  assert.deepEqual(gestureDrift(complete, complete), []);
  // The editor gives a new node its own id, a new edge the node's first port,
  // and a new loop region its own id: none of it shows on the canvas.
  const saved = structuredClone(built);
  const node = saved.nodes.find((n) => n.id === "implementer");
  node.id = "node-3";
  node.outputs = [{ name: "out", side: "right" }];
  delete node.pin_harness;
  node.isolated_worktree = false;
  for (const e of saved.edges) {
    if (e.source.node === "implementer") e.source = { node: "node-3", port: "out" };
    if (e.target.node === "implementer") e.target = { node: "node-3", port: "user_prompt" };
  }
  node.view = { x: node.view.x + 3, y: node.view.y - 2 };
  assert.deepEqual(gestureDrift(saved, built), []);
  const loop = structuredClone(complete);
  loop.loops[0].id = "loop-0123456789abcdef";
  loop.edges[2].waypoints = [{ x: 452, y: 162 }]; // the same bend, drawn once
  assert.deepEqual(gestureDrift(loop, complete), []);
});

test("the drift reads the wire the canvas draws, not the waypoints it was saved with", () => {
  // routing/a, FP of #882: the loop edge saved with one bend instead of the
  // target's two, pressed one px higher on reviewer's rim. The canvas adds the
  // leg out of reviewer's right and squares the bend: the same wire.
  const saved = structuredClone(complete);
  const loop = saved.edges.find((e) => edgeKey(e) === "reviewer→implementer");
  loop.waypoints = [{ x: 452, y: 275 }];
  loop.source_anchor = { side: "right", offset: 39 };
  assert.deepEqual(gestureDrift(saved, complete), []);
  assert.deepEqual(drawnRoute([{ x: 357, y: 234 }, { x: 397, y: 154 }, { x: 396, y: 154 }, { x: 356, y: 154 }], "right", "right"), [
    { x: 357, y: 234 },
    { x: 397, y: 234 },
    { x: 397, y: 154 },
    { x: 396, y: 154 },
    { x: 356, y: 154 },
  ]);
  // A wire that really goes elsewhere still drifts: the loop squared on the left.
  loop.waypoints = [{ x: 150, y: 276 }, { x: 150, y: 162 }];
  assert.equal(gestureDrift(saved, complete).filter((l) => l.startsWith("edge reviewer→implementer: route")).length, 1);
});

test("the cursor draws an edge along its wire, one axis per move, without a reversal", () => {
  const wire = drawnRoute([{ x: 357, y: 234 }, { x: 397, y: 154 }, { x: 396, y: 154 }, { x: 356, y: 154 }], "right", "right");
  const path = cursorPath(wire, { x: 354, y: 234 }, { x: 351, y: 154 });
  assert.deepEqual(path, [{ x: 354, y: 234 }, { x: 397, y: 234 }, { x: 397, y: 154 }, { x: 351, y: 154 }]);
  // Start → implementer: the leg out of Start overshoots the pinned bend, which
  // the canvas merges away; the cursor goes straight down.
  const straight = drawnRoute([{ x: 280, y: 82 }, { x: 280, y: 95 }, { x: 280, y: 135 }], "bottom", "top");
  assert.deepEqual(cursorPath(straight, { x: 280, y: 79 }, { x: 280, y: 140 }), [{ x: 280, y: 79 }, { x: 280, y: 140 }]);
  for (const [a, b] of path.slice(1).map((p, i) => [path[i], p])) assert.ok(a.x === b.x || a.y === b.y);
});

test("a gesture that drifts from the target is recorded, one line per visible difference", () => {
  const saved = structuredClone(complete);
  const loopEdge = saved.edges.find((e) => edgeKey(e) === "reviewer→implementer");
  loopEdge.condition_label_pos = { x: loopEdge.condition_label_pos.x + 40, y: loopEdge.condition_label_pos.y };
  loopEdge.source = { node: "reviewer", port: "review" };
  const exit = saved.edges.find((e) => edgeKey(e) === "reviewer→end");
  exit.when = { verdict: { neq: "fail" } };
  exit.target_anchor = { side: "left", offset: 20 };
  saved.nodes.find((n) => n.id === "end").view = { x: 204, y: 360 };
  saved.loops[0].max_iter = 3;
  delete saved.nodes.find((n) => n.id === "reviewer").isolated_worktree;
  const drift = gestureDrift(saved, complete);
  assert.deepEqual(drift, [
    "node end: at (204, 360) vs target (272, 424)",
    "node reviewer: isolated vs target shares the run worktree",
    "edge reviewer→implementer: carries review vs target review, screenshots",
    "edge reviewer→implementer: condition label at (492, 216) vs target (452, 216)",
    'edge reviewer→end: condition {"verdict":{"neq":"fail"}} vs target {"verdict":{"eq":"pass"}}',
    "edge reviewer→end: target_anchor on left vs target top",
    "edge reviewer→end: route (300, 301) (300, 341) (300, 380) (164, 380) (204, 380) vs target (300, 301) (300, 341) (300, 384) (300, 424)",
    "loop region implementer,reviewer: bounded ↻ 3 vs target bounded ↻ 5",
  ]);
  // Under the tolerance, a position reads the same.
  const close = structuredClone(complete);
  close.nodes[0].view = { x: close.nodes[0].view.x + DRIFT_TOLERANCE - 1, y: close.nodes[0].view.y };
  assert.deepEqual(gestureDrift(close, complete), []);
  // What is missing and what is extra.
  const bare = startState(complete, { edges: ["reviewer→implementer"] });
  bare.edges.push({ source: { node: "end", port: "x" }, target: { node: "start", port: "y" } });
  assert.deepEqual(gestureDrift(bare, complete), [
    "edge reviewer→implementer: missing",
    "edge end→start: not in the target",
    "loop region implementer,reviewer: missing",
  ]);
});

test("the manifest records a simulated drift as a warning, without failing the variant", () => {
  const saved = structuredClone(built);
  saved.nodes.find((n) => n.id === "implementer").view = { x: 260, y: 135 };
  const entry = variantEntry({
    variant: { id: "a", label: "a", markers: ["node-added"] },
    facts: { durationS: 10, width: 960, height: 700, bytes: 1 },
    plan: { markers: [{ name: "node-added", t: 1000 }] },
    gif: "pipelines/a.gif",
    poster: "pipelines/a.jpg",
    posterBytes: 1,
    target: TARGET_MS,
    sceneWarnings: driftWarnings(gestureDrift(saved, built)),
  });
  // The card moved, and the ends of its two edges with it.
  assert.deepEqual(entry.warnings, [
    "drift from the target: node implementer: at (260, 135) vs target (196, 130)",
    "drift from the target: edge start→implementer: route (300, 84) (300, 124) (300, 95) (364, 95) (364, 135) vs target (300, 84) (300, 124) (300, 90) (300, 130)",
    "drift from the target: edge implementer→end: route (364, 200) (364, 240) (300, 240) (300, 210) (300, 250) vs target (300, 195) (300, 235) (300, 210) (300, 250)",
  ]);
  assert.equal(entry.gif, "pipelines/a.gif");
});

test("the building scenes declare what they draw, and both variants of each", () => {
  assert.deepEqual(scenes.get("pipelines").drawn, { nodes: ["implementer"] });
  assert.deepEqual(scenes.get("routing").drawn, { edges: ["reviewer→implementer"], conditions: ["reviewer→end"] });
  for (const name of ["pipelines", "routing"]) assert.equal(scenes.get(name).variants.length, 2);
  const variant = (id) => ({ id, viewport: { width: 1, height: 1 }, markers: ["m"], play() {} });
  const scene = (drawn) => ({ name: "x", drawn, variants: [variant("a"), variant("b")] });
  assert.throws(() => validateScene(scene({ node: ["implementer"] }), "x.mjs"), /`drawn` lists/);
  assert.throws(() => validateScene(scene({ edges: "reviewer→implementer" }), "x.mjs"), /`drawn` lists/);
  validateScene(scene({ nodes: [], edges: [], conditions: [] }), "x.mjs");
  // The start state installs as YAML that parses back to the same pipeline.
  for (const [name, target] of [["pipelines", built], ["routing", complete]]) {
    const start = startState(target, scenes.get(name).drawn);
    assert.deepEqual(parseYaml(dumpYaml(start)), start);
  }
});

test("the canvas draws what lies under a card with several outputs lower, by the card's growth", () => {
  // reviewer lists two outputs: drawn 81 px tall for the 65 it was placed for,
  // so everything from its placed bottom (236 + 65) down is drawn 16 px lower.
  const down = drawnY(complete);
  const reviewer = complete.nodes.find((n) => n.id === "reviewer").view;
  assert.equal(down(complete.nodes.find((n) => n.id === "start").view.y), 60);
  assert.equal(down(reviewer.y), reviewer.y);
  assert.equal(down(reviewer.y + 64), reviewer.y + 64);
  assert.equal(down(reviewer.y + 65), reviewer.y + 65 + 16);
  assert.equal(down(complete.nodes.find((n) => n.id === "end").view.y), complete.nodes.find((n) => n.id === "end").view.y + 16);
  // Cards of one output each: drawn as stored.
  assert.equal(drawnY(built)(400), 400);
});
