// Helpers for the scenes filmed on the EDIT canvas of the demo pipeline
// `implement-review` (pipelines, routing). Not a scene: the `_` prefix keeps it
// out of scene discovery.

import fs from "node:fs";
import path from "node:path";
import { sleep } from "../lib/demo-instance.mjs";
import { DEMO_PIPELINE_ID, installTarget, parseYaml, readTarget, target, withName } from "../lib/targets.mjs";

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

/** A port's handle on a canvas node (`node` is a locator of the node). */
export function handle(node, port) {
  return node.locator(`.react-flow__handle[data-handleid="${port}"]`).first();
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
