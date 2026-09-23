// Helpers for the scenes filmed on the EDIT canvas of the demo pipeline
// `implement-review` (pipelines, routing). Not a scene: the `_` prefix keeps it
// out of scene discovery.

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { sleep } from "../lib/demo-instance.mjs";
import { DEMO_PIPELINE } from "../lib/demo-pipeline.mjs";

const FIXTURE_PIPELINE = fileURLToPath(new URL(`../fixture/pipelines/${DEMO_PIPELINE.id}.yaml`, import.meta.url));

/** Replace the demo pipeline in the instance's library with `yaml` (same name:
 *  the one pipeline any GIF shows), and wait until the daemon serves it with
 *  `nodes` nodes and `edges` edges. A variant that saves its canvas changes the
 *  library, so each variant installs its starting pipeline before it plays. */
export async function installPipeline(instance, yaml, { nodes, edges }) {
  const file = path.join(instance.home, ".pdo", "pipelines", `${DEMO_PIPELINE.id}.yaml`);
  if (fs.readFileSync(file, "utf8") !== yaml) {
    fs.writeFileSync(file, yaml);
    // Let the library watcher's change event land before a tab opens on the
    // pipeline: arriving after, it would mark the tab « changed on disk ».
    await sleep(2500);
  }
  const deadline = Date.now() + 15_000;
  let seen = null;
  while (Date.now() < deadline) {
    const { pipeline } = await instance.api("GET", `/pipelines/${DEMO_PIPELINE.id}`);
    seen = { nodes: pipeline.nodes?.length, edges: pipeline.edges?.length ?? 0 };
    if (seen.nodes === nodes && seen.edges === edges) return;
    await sleep(200);
  }
  throw new Error(`the daemon never served ${DEMO_PIPELINE.id} with ${nodes} nodes / ${edges} edges (last: ${JSON.stringify(seen)})`);
}

/** Put the demo pipeline back as versioned (fixture/pipelines/): start →
 *  implementer → reviewer → end. */
export async function restoreDemoPipeline(instance) {
  await installPipeline(instance, fs.readFileSync(FIXTURE_PIPELINE, "utf8"), { nodes: 4, edges: 3 });
}

/** Open the demo pipeline on the edit canvas, off camera. */
export async function openPipeline(ctx) {
  const { page } = ctx;
  await ctx.goto("/");
  // Through ctx, even off camera: the recorder must know where the cursor is.
  await ctx.click(page.getByTestId("left-tab-library"), { duration: 150, pause: 40 });
  await ctx.click(page.getByTestId(`library-row-${DEMO_PIPELINE.id}`), { duration: 150, pause: 40 });
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
