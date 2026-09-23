// Scene « Conditional routing & loops » (README row 2). No live agent. On the
// edit canvas of the demo pipeline `implement-review`, from its complete target
// (fixture/targets/implement-review.yaml, #882): the take starts from the
// target without its loop edge (and so without the loop region) and without
// the exit's condition. The cursor draws the loop edge `reviewer → implementer`
// from reviewer's rim to implementer's anchor (#840) — PDO wraps the two nodes
// in the loop region ↻ 5 — sets its condition `review.verdict = fail` and drags
// the pill to its place, then sets `verdict = pass` on the exit to End and drags
// that pill to its own. Saved; the drift from the target goes to the manifest,
// and the target is reinstalled off camera: the poster is the maintainer's
// drawing.
//
//   a — (the published one) focused on the loop, camera zoomed in: the drag,
//       then each condition lands on the canvas. The inspector is out of frame.
//   b — the same, with the edge inspector in frame: the loop condition's value
//       is picked in the panel.

import { sleep } from "../lib/demo-instance.mjs";
import { targetPipeline } from "../lib/targets.mjs";
import {
  choose,
  clearToolbar,
  dragLabelTo,
  drawEdge,
  emptyCanvasSpot,
  installStartState,
  landOnTarget,
  openPipeline,
  pointOnEdge,
  saveCanvas,
  zoomCanvas,
} from "./_canvas.mjs";

const TARGET = "implement-review";
/** What the gestures draw: the loop edge (its region comes with it) and the exit's condition. */
const DRAWN = { edges: ["reviewer→implementer"], conditions: ["reviewer→end"] };

const quick = { duration: 150, pause: 40 };

/** The target edge `from → to`. */
function edgeOf(target, from, to) {
  return target.edges.find((e) => e.source.node === from && e.target.node === to);
}

/** Select an edge by clicking on its drawn route. */
async function selectEdge(ctx, id, { film, at = 0.5 }) {
  const { page } = ctx;
  await ctx.click(await pointOnEdge(page, id, at), film ? { duration: 650 } : quick);
  await page.getByTestId("edge-detail-panel").waitFor();
  await sleep(film ? 400 : 250);
}

/** Add a condition row to the selected edge: `port` (a multi-port edge names
 *  the carried output), field `verdict`, `op`, `value`. `film` moves the cursor
 *  through the panel; otherwise the page picks the values. */
async function setCondition(ctx, { port, op, value, film }) {
  const { page } = ctx;
  const panel = page.getByTestId("edge-detail-panel");
  const add = panel.getByTestId("add-condition");
  await add.waitFor();
  await ctx.click(add, film ? { duration: 550 } : quick);
  const row = panel.getByTestId("condition-row").first();
  await row.waitFor();
  await sleep(250);
  // A control already on its value (the row defaults to the first port, its
  // only field, `=`) is left alone: the camera only sees the picks that matter.
  const pick = async (select, v) => {
    if ((await select.inputValue()) === v) return;
    await (film ? choose(ctx, select, v) : select.selectOption(v));
  };
  if (port) await pick(row.getByTestId("condition-port-dropdown"), port);
  await pick(row.getByTestId("field-dropdown"), "verdict");
  await pick(row.getByTestId("op-dropdown"), op);
  await pick(row.getByTestId("value-dropdown"), value);
  await sleep(400);
}

/** The loop edge carries both of reviewer's outputs, as drawn: a drawn edge
 *  carries the first one only. Off camera, then its output tags go to the
 *  target's places. */
async function carryBothOutputs(ctx, id, loop) {
  const { page } = ctx;
  const panel = page.getByTestId("edge-detail-panel");
  const box = panel.getByTestId("output-checkbox-screenshots").locator("input");
  if (!(await box.isChecked())) await ctx.click(box, quick);
  await page.getByTestId(`edge-output-label-${id}-screenshots`).waitFor();
  for (const [port, pos] of Object.entries(loop.output_label_pos ?? {})) {
    await dragLabelTo(ctx, page.getByTestId(`edge-output-label-${id}-${port}`), pos, { duration: 200, film: false });
  }
}

async function play(ctx, { film, zoom }) {
  const { page } = ctx;
  const target = targetPipeline(TARGET);
  const loop = edgeOf(target, "reviewer", "implementer");
  const exit = edgeOf(target, "reviewer", "end");
  const start = await installStartState(ctx.instance, TARGET, DRAWN);
  // The canvas names an edge `e-<index>`: the exit keeps its index, the loop edge is appended.
  const exitId = `e-${start.edges.findIndex((e) => e.source.node === "reviewer" && e.target.node === "end")}`;
  const loopId = `e-${start.edges.length}`;
  await openPipeline(ctx);
  if (zoom) {
    // Off camera: the camera zooms on implementer → reviewer, and the loop's room on their right.
    const a = await page.getByTestId("rf__node-implementer").boundingBox();
    const b = await page.getByTestId("rf__node-reviewer").boundingBox();
    await zoomCanvas(ctx, { x: a.x + a.width / 2 + 30, y: (a.y + b.y + b.height) / 2 + 25 });
  }
  await clearToolbar(ctx);
  await ctx.moveTo(await emptyCanvasSpot(page), { duration: 300 });

  // 1. The loop edge, from reviewer's rim back to implementer: the region ↻ 5 appears.
  await ctx.hold(300);
  await drawEdge(ctx, loop, { duration: 1200 });
  await page.getByTestId("loop-region").waitFor();
  await ctx.hold(400);
  ctx.mark("edge-dropped", { before: 200, after: 400 });

  // 2. Its condition, `review.verdict = fail`, then the pill to its place.
  await selectEdge(ctx, loopId, { film: false, at: 0.5 });
  await carryBothOutputs(ctx, loopId, loop);
  const [lop, lvalue] = Object.entries(Object.values(loop.when)[0])[0];
  if (film) await ctx.keep(() => setCondition(ctx, { port: "review", op: lop, value: lvalue, film: true }));
  else await setCondition(ctx, { port: "review", op: lop, value: lvalue, film: false });
  await ctx.moveTo(await emptyCanvasSpot(page), quick);
  await dragLabelTo(ctx, page.getByTestId(`edge-condition-label-${loopId}`), loop.condition_label_pos, { duration: 700 });
  ctx.mark("condition-set", { before: 200, after: 500 });

  // 3. The exit: `verdict = pass`, then its pill to its place.
  await selectEdge(ctx, exitId, { film: false, at: 0.4 });
  const [eop, evalue] = Object.entries(Object.values(exit.when)[0])[0];
  // The panel gesture was shown once (b, on the loop edge): this one is set off camera.
  await setCondition(ctx, { op: eop, value: evalue, film: false });
  await ctx.moveTo(await emptyCanvasSpot(page), quick);
  await dragLabelTo(ctx, page.getByTestId(`edge-condition-label-${exitId}`), exit.condition_label_pos, { duration: 700 });
  const spot = await emptyCanvasSpot(page);
  await ctx.keep(async () => {
    await ctx.click(spot, { duration: 500 });
    await sleep(300);
  });
  ctx.mark("condition-saved", { before: 200, after: 200 });

  // Off camera: save, record the drift, the target back in place.
  await saveCanvas(ctx);
  await landOnTarget(ctx, TARGET);
  await ctx.hold(1800);
}

export default {
  name: "routing",
  title: "Conditional routing & loops",
  needs: [],
  live: [],
  drawn: DRAWN,
  variants: [
    {
      id: "a",
      label: "Loop edge reviewer → implementer, then verdict = fail and verdict = pass land on the canvas (zoomed on the loop)",
      viewport: { width: 1100, height: 760 },
      localStorage: { "pdo.layout.run": { left: 17, center: 52, right: 31 } },
      // The canvas' full width, from above Start to below End; the toolbar is above the frame.
      crop: { x: 188, y: 150, width: 570, height: 470 },
      markers: ["edge-dropped", "condition-set", "condition-saved"],
      play: (ctx) => play(ctx, { film: false, zoom: true }),
    },
    {
      id: "b",
      label: "The same, with the edge inspector: the loop condition picked in the panel",
      viewport: { width: 1100, height: 760 },
      localStorage: { "pdo.layout.run": { left: 17, center: 44, right: 39 } },
      // Canvas and inspector, from the tab bar (the edge panel's header reads whole).
      crop: { x: 188, y: 44, width: 912, height: 656 },
      markers: ["edge-dropped", "condition-set", "condition-saved"],
      play: (ctx) => play(ctx, { film: true, zoom: true }),
    },
  ],
};
