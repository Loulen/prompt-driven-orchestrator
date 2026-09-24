// Helpers for the scenes filmed on the EDIT canvas of the demo pipeline
// `implement-review` (pipelines, routing). Not a scene: the `_` prefix keeps it
// out of scene discovery.

import fs from "node:fs";
import path from "node:path";
import { sleep } from "../lib/demo-instance.mjs";
import { cursorPath, drawnRoute, driftWarnings, gestureDrift, startState } from "../lib/build-scene.mjs";
import { DEMO_PIPELINE_ID, dumpYaml, installTarget, parseYaml, readTarget, target, targetPipeline, withName } from "../lib/targets.mjs";

/** Replace `implement-review` in the instance's library with `yaml`, and wait
 *  until the daemon serves it with `nodes` nodes and `edges` edges. A variant
 *  that saves its canvas changes the library, so each variant installs its
 *  starting pipeline before it plays. */
export async function installPipeline(instance, yaml, { nodes, edges }) {
  const file = path.join(instance.home, ".pdo", "pipelines", `${DEMO_PIPELINE_ID}.yaml`);
  if (fs.readFileSync(file, "utf8") !== yaml) {
    fs.writeFileSync(file, yaml);
    await settle();
  }
  await waitServed(instance, DEMO_PIPELINE_ID, { nodes, edges });
}

/** Install a target pipeline (lib/targets.mjs) as is — `yaml` overrides its
 *  text, e.g. with a node flag on — prompts included, and wait until the
 *  daemon serves it. */
export async function installTargetPipeline(instance, id, { yaml } = {}) {
  const file = path.join(instance.libraryDir, `${target(id).name}.yaml`);
  const before = fs.existsSync(file) ? fs.readFileSync(file, "utf8") : null;
  const installed = installTarget(instance.libraryDir, id, { yaml });
  if (installed !== before) await settle();
  const pipeline = parseYaml(installed);
  await waitServed(instance, target(id).name, { nodes: pipeline.nodes.length, edges: pipeline.edges?.length ?? 0 });
  return installed;
}

/** Let the library watcher's change event land before a tab opens on the
 *  pipeline: arriving after, it would mark the tab « changed on disk ». */
function settle() {
  return sleep(2500);
}

async function waitServed(instance, id, { nodes, edges }) {
  const deadline = Date.now() + 15_000;
  let seen = null;
  while (Date.now() < deadline) {
    const { pipeline } = await instance.api("GET", `/pipelines/${id}`);
    seen = { nodes: pipeline.nodes?.length, edges: pipeline.edges?.length ?? 0 };
    if (seen.nodes === nodes && seen.edges === edges) return;
    await sleep(200);
  }
  throw new Error(`the daemon never served ${id} with ${nodes} nodes / ${edges} edges (last: ${JSON.stringify(seen)})`);
}

/** Install the start state of a building scene: target `id` without what the
 *  scene draws (lib/build-scene.mjs), under the demo name. Returns it. */
export async function installStartState(instance, id, drawn) {
  const start = startState(targetPipeline(id), drawn);
  const yaml = withName(dumpYaml(start), target(id).name);
  await installPipeline(instance, yaml, { nodes: start.nodes.length, edges: start.edges.length });
  return start;
}

/** Put the demo pipeline back as its target: the complete `implement-review`,
 *  `implementer → reviewer` with its loop. */
export async function restoreDemoPipeline(instance) {
  await installTargetPipeline(instance, DEMO_PIPELINE_ID);
}

/** The YAML of a target as the demo instance installs it (its demo `name:`). */
export function targetYaml(id) {
  return withName(readTarget(id).yaml, target(id).name);
}

/** Open the demo pipeline on the edit canvas, off camera. */
export async function openPipeline(ctx) {
  const { page } = ctx;
  await ctx.goto("/");
  // Through ctx, even off camera: the recorder must know where the cursor is.
  await ctx.click(page.getByTestId("left-tab-library"), { duration: 150, pause: 40 });
  await ctx.click(page.getByTestId(`library-row-${DEMO_PIPELINE_ID}`), { duration: 150, pause: 40 });
  await page.getByTestId("rf__node-start").waitFor({ timeout: 30_000 });
  await sleep(700);
}

/** A point `at` (0..1) of the way along an edge's drawn path, in page px. The
 *  edge's hit box is a rectangle around the whole route, so its centre is not
 *  on the edge; this is. */
export async function pointOnEdge(page, edgeId, at = 0.5) {
  return page.getByTestId(`rf__edge-${edgeId}`).evaluate((g, k) => {
    const route = [...g.querySelectorAll("path")].sort((a, b) => b.getTotalLength() - a.getTotalLength())[0];
    const p = route.getPointAtLength(route.getTotalLength() * k);
    const m = route.getScreenCTM();
    return { x: p.x * m.a + p.y * m.c + m.e, y: p.x * m.b + p.y * m.d + m.f };
  }, at);
}

/** The camera zooms the canvas around `at` (a point or a locator): the wheel
 *  zooms React Flow around the cursor (-240 ≈ ×1.4). */
export async function zoomCanvas(ctx, at, { dy = -240, duration = 800 } = {}) {
  await ctx.moveTo(at, { duration: 500 });
  await ctx.scroll(dy, { duration });
  await sleep(450);
}

/** Pick a value in a native <select>: the cursor clicks it, the page picks. */
export async function choose(ctx, select, value) {
  await ctx.click(select, { duration: 550 });
  await sleep(200);
  await select.selectOption(value);
  await sleep(250);
}

/** An empty spot of the canvas (bottom-left corner), to deselect. */
export async function emptyCanvasSpot(page) {
  const box = await page.getByTestId("rf__wrapper").boundingBox();
  return { x: box.x + 28, y: box.y + box.height - 28 };
}

// ---- building from the target (#882) -----------------------------------------
//
// A scene that builds (pipelines, routing) installs `startState(target, drawn)`
// (lib/build-scene.mjs), films gestures aimed at the target's own coordinates
// (flow px: node `view`, edge anchors, waypoints, label positions), saves, then
// `landOnTarget` records the drift and reinstalls the target off camera.

/** The canvas viewport: pan (screen px) and zoom. */
export async function canvasViewport(page) {
  const transform = await page.locator(".react-flow__viewport").first().evaluate((el) => el.style.transform);
  const m = transform.match(/translate\(([-\d.]+)px,\s*([-\d.]+)px\)\s*scale\(([-\d.]+)\)/);
  if (!m) throw new Error(`unreadable canvas viewport: ${transform}`);
  const pane = await page.getByTestId("rf__wrapper").boundingBox();
  return { x: pane.x + Number(m[1]), y: pane.y + Number(m[2]), zoom: Number(m[3]) };
}

/** A flow point on screen (page px). */
export async function toScreen(page, point) {
  const vp = await canvasViewport(page);
  return { x: vp.x + point.x * vp.zoom, y: vp.y + point.y * vp.zoom };
}

/** A node's card in flow px (`x`, `y` = its `view`), measured on the canvas. */
export async function nodeRect(page, nodeId) {
  const vp = await canvasViewport(page);
  const box = await page.getByTestId(`rf__node-${nodeId}`).boundingBox();
  if (!box) throw new Error(`node ${nodeId} is not on the canvas`);
  return { x: (box.x - vp.x) / vp.zoom, y: (box.y - vp.y) / vp.zoom, width: box.width / vp.zoom, height: box.height / vp.zoom };
}

/** The measured size of every node of `pipeline` on the canvas (flow px), for gestureDrift. */
export async function nodeSizes(page, pipeline) {
  const sizes = {};
  for (const node of pipeline.nodes) {
    if ((await page.getByTestId(`rf__node-${node.id}`).count()) === 0) continue;
    const { width, height } = await nodeRect(page, node.id);
    sizes[node.id] = { width, height };
  }
  return sizes;
}

/** The flow point of an anchor (`{ side, offset }`) on a card, moved `inset`
 *  flow px into the card (a press on the rim strip, a drop on the body). */
export function anchorPoint(rect, { side, offset }, inset = 0) {
  switch (side) {
    case "top":
      return { x: rect.x + offset, y: rect.y + inset };
    case "bottom":
      return { x: rect.x + offset, y: rect.y + rect.height - inset };
    case "left":
      return { x: rect.x + inset, y: rect.y + offset };
    case "right":
      return { x: rect.x + rect.width - inset, y: rect.y + offset };
  }
  throw new Error(`unknown side ${side}`);
}

// The gestures below measure first, then run the gesture itself in a
// `ctx.keep` (`film: false`: off camera): the measuring round trips never land
// in the GIF.

/** Keep `fn` on camera, or run it off camera. */
const filmed = (ctx, film, fn) => (film ? ctx.keep(fn) : fn());

/** Move the cursor through screen points, pressed (a drag along a route). */
async function dragThrough(ctx, screen, { duration }) {
  const { page } = ctx;
  const lengths = screen.slice(1).map((p, i) => Math.hypot(p.x - screen[i].x, p.y - screen[i].y));
  const total = lengths.reduce((a, b) => a + b, 0) || 1;
  await ctx.moveTo(screen[0], { duration: 400 });
  await sleep(120);
  await page.mouse.down();
  await sleep(120);
  for (const [i, p] of screen.slice(1).entries()) await ctx.moveTo(p, { duration: Math.max(120, (duration * lengths[i]) / total) });
  await sleep(120);
  await page.mouse.up();
}

/**
 * Draw `edge` (a target edge) the way #840 wires it: pressed on the source
 * card's rim at its `source_anchor`, dragged along the wire the canvas draws for
 * it (its legs and squared waypoints, `drawnRoute`), released on the target card
 * at its `target_anchor` — the drop point is what anchors it. The cursor never
 * cuts a corner: a diagonal move let the editor's grid trace pick its bends by
 * pointer sampling, a different route from one take to the next.
 */
export async function drawEdge(ctx, edge, { duration = 850, film = true } = {}) {
  const { page } = ctx;
  const from = await nodeRect(page, edge.source.node);
  const to = await nodeRect(page, edge.target.node);
  const bends = edge.mode === "manual" ? (edge.waypoints ?? []) : [];
  const [src, tgt] = [anchorPoint(from, edge.source_anchor), anchorPoint(to, edge.target_anchor)];
  const wire = drawnRoute([src, ...bends, tgt], edge.source_anchor.side, edge.target_anchor.side);
  const points = cursorPath(wire, anchorPoint(from, edge.source_anchor, 3), anchorPoint(to, edge.target_anchor, 5));
  const screen = [];
  for (const p of points) screen.push(await toScreen(page, p));
  await filmed(ctx, film, async () => {
    await dragThrough(ctx, screen, { duration });
    await sleep(300);
  });
}

/** The px the card lags the pointer by at the end of a drag (xyflow's drag threshold, measured). */
const DRAG_THRESHOLD = 3;

/** Drag a node by its card toward `view` (flow px), with the pointer. */
async function dragCard(ctx, nodeId, view, { duration, film }) {
  const { page } = ctx;
  const rect = await nodeRect(page, nodeId);
  // Grab the card's middle-left, clear of the rim strips (the edge sources).
  const grab = { x: rect.x + rect.width * 0.35, y: rect.y + rect.height / 2 };
  // xyflow starts moving the card only once the pointer is past its drag
  // threshold, and the card never gets those first px back: travel them twice.
  const d = { x: view.x - rect.x, y: view.y - rect.y };
  const len = Math.hypot(d.x, d.y) || 1;
  const k = (len + DRAG_THRESHOLD) / len;
  const from = await toScreen(page, grab);
  const drop = await toScreen(page, { x: grab.x + d.x * k, y: grab.y + d.y * k });
  await filmed(ctx, film, async () => {
    await ctx.moveTo(from, { duration: Math.min(600, duration) });
    await page.mouse.down();
    await sleep(120);
    await ctx.moveTo(drop, { duration });
    await sleep(120);
    await page.mouse.up();
    await sleep(250);
  });
}

/** Drag a node by its card to its target `view` (flow px). */
export async function dragNodeTo(ctx, nodeId, view, { duration = 900, film = true } = {}) {
  await dragCard(ctx, nodeId, view, { duration, film });
}

/**
 * The card lags the pointer by a variable few px (xyflow's drag threshold):
 * short nudges, off camera, until it sits on `view`.
 */
export async function nudgeNodeTo(ctx, nodeId, view, { tolerance = 1 } = {}) {
  for (let i = 0; i < 6; i++) {
    const rect = await nodeRect(ctx.page, nodeId);
    if (Math.abs(rect.x - view.x) <= tolerance && Math.abs(rect.y - view.y) <= tolerance) return;
    await dragCard(ctx, nodeId, view, { duration: 200, film: false });
  }
}

/** Drag a label (a locator: a condition pill, an output tag) so its centre
 *  lands on `pos` (flow px), where the editor pins a dragged label. */
export async function dragLabelTo(ctx, label, pos, { duration = 800, film = true } = {}) {
  const from = await ctx.point(label);
  const to = await toScreen(ctx.page, pos);
  await filmed(ctx, film, async () => {
    await ctx.moveTo(from, { duration: 450 });
    await ctx.page.mouse.down();
    await sleep(100);
    await ctx.moveTo(to, { duration });
    await sleep(100);
    await ctx.page.mouse.up();
    await sleep(300);
  });
}

/** Save the open pipeline (Ctrl+S) and wait until the editor says so. */
export async function saveCanvas(ctx) {
  await ctx.page.keyboard.press("Control+s");
  await ctx.page.getByText("Saved just now").waitFor({ timeout: 10_000 });
  ctx.savedAt = Date.now();
  await sleep(300);
}

/** How long the daemon takes a change to a file it just wrote for its own (SELF_WRITE_TTL), plus a beat. */
const SELF_WRITE_TTL_MS = 2500;

/**
 * The end of a building scene, off camera (#882, Q41): compare what the
 * gestures SAVED to the target, record each visible difference as a manifest
 * warning (the take is kept), then reinstall the target and wait until the open
 * tab has reloaded it cleanly. The final shot and the poster are the target.
 * Returns the drift.
 */
export async function landOnTarget(ctx, id) {
  const { page, instance } = ctx;
  const target = targetPipeline(id);
  const file = path.join(instance.libraryDir, `${DEMO_PIPELINE_ID}.yaml`);
  const saved = parseYaml(fs.readFileSync(file, "utf8"));
  const drift = gestureDrift(saved, target, { sizes: await nodeSizes(page, saved) });
  for (const warning of driftWarnings(drift)) ctx.warn(warning);
  if (drift.length > 0) console.warn(`   drift from the target ${id}:\n   - ${drift.join("\n   - ")}`);
  // The daemon ignores a change to a file it wrote itself within 2 s (its own
  // save): reinstalled sooner, the target would never reach the open tab.
  await sleep(Math.max(0, SELF_WRITE_TTL_MS - (Date.now() - ctx.savedAt)));
  await installTargetPipeline(instance, id);
  // The tab hot-reloads a clean pipeline: every node of the target at its
  // `view`, under its own id. Then the tab's « changed » dot fades (2 s).
  await page.waitForFunction(
    (nodes) =>
      nodes.every(({ id, x, y }) => {
        const el = document.querySelector(`.react-flow__node[data-id="${id}"]`);
        return el && el.style.transform.replace(/\s/g, "") === `translate(${x}px,${y}px)`;
      }),
    target.nodes.map((n) => ({ id: n.id, x: n.view.x, y: n.view.y })),
    { timeout: 15_000 },
  );
  await sleep(2600);
  return drift;
}

/**
 * Keep every node clear of the canvas toolbar (#882): when a card or a loop
 * region comes closer than `margin` px to the toolbar's bottom, pan the canvas
 * down, off camera. Call it once the viewport is set (after a zoom).
 */
export async function clearToolbar(ctx, { margin = 24 } = {}) {
  const { page } = ctx;
  const toolbar = await page.getByTestId("toolbar-add").locator("xpath=..").boundingBox();
  const tops = await page.locator(".react-flow__node").evaluateAll((els) => els.map((el) => el.getBoundingClientRect().top));
  const top = Math.min(...tops);
  const gap = top - (toolbar.y + toolbar.height);
  if (gap >= margin) return;
  const pane = await page.getByTestId("rf__wrapper").boundingBox();
  const from = { x: pane.x + pane.width - 30, y: pane.y + pane.height / 2 };
  await ctx.drag(from, { x: from.x, y: from.y + (margin - gap) }, { duration: 200 });
  await sleep(300);
}

/**
 * Keep the span `left`..`right` (flow px: what the take will draw, labels
 * included) clear of the canvas' side edges (#882): when either end comes closer
 * than `margin` px to its edge, pan the canvas, off camera, to centre the span.
 * The canvas meets the inspector on its right, where a pill dragged near the
 * edge came out clipped. Call it once the viewport is set (after a zoom).
 */
export async function clearSides(ctx, { left, right }, { margin = 24 } = {}) {
  const { page } = ctx;
  const pane = await page.getByTestId("rf__wrapper").boundingBox();
  const [a, b] = [await toScreen(page, { x: left, y: 0 }), await toScreen(page, { x: right, y: 0 })];
  if (a.x - pane.x >= margin && pane.x + pane.width - b.x >= margin) return;
  const dx = Math.round(pane.x + (pane.width - (b.x - a.x)) / 2 - a.x);
  // From an empty corner at the bottom, on the side the canvas moves away from.
  const from = { x: dx < 0 ? pane.x + pane.width - 30 : pane.x + 30, y: pane.y + pane.height - 28 };
  await ctx.drag(from, { x: from.x + dx, y: from.y }, { duration: 200 });
  await sleep(300);
}
