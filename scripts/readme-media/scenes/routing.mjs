// Scene « Conditional routing & loops » (README row 2). No live agent: on the
// edit canvas of the demo pipeline `implement-review` (implementer → reviewer),
// the cursor drags an edge from `reviewer` back to `implementer` — PDO wraps
// the two nodes in a loop region (↻ max 5) — and sets its condition
// `verdict != pass`. The exit to End becomes the default branch (`else`), so
// only a passing review leaves the loop. Then the pipeline is saved (Ctrl+S).
//
//   a — (the published one) focused on the loop, camera zoomed in: the drag,
//       then the condition lands on the canvas. The inspector is out of frame.
//   b — the whole gesture at 1:1, canvas and edge inspector: the drag, the
//       click on the new edge, the condition's operator and value.

import { sleep } from "../lib/demo-instance.mjs";
import { choose, emptyCanvasSpot, handle, openPipeline, pointOnEdge, restoreDemoPipeline, zoomCanvas } from "./_canvas.mjs";

/** Drag `reviewer.review` back onto `implementer` (the new edge is `e-3`). */
async function dragLoopEdge(ctx) {
  const { page } = ctx;
  const implementer = page.getByTestId("rf__node-implementer");
  const box = await implementer.boundingBox();
  await ctx.drag(handle(page.getByTestId("rf__node-reviewer"), "review"), { x: box.x + box.width * 0.72, y: box.y + box.height / 2 }, { duration: 1100 });
  await page.getByTestId("loop-region").waitFor();
  await sleep(700);
}

/** The loop edge's condition in the edge inspector: `verdict` ≠ `pass`.
 *  `film` moves the cursor through it; otherwise the page picks the values. */
async function setCondition(ctx, { film }) {
  const { page } = ctx;
  const panel = page.getByTestId("edge-detail-panel");
  const add = panel.getByTestId("add-condition");
  await add.waitFor();
  await ctx.click(add, { duration: film ? 700 : 150 });
  const row = panel.getByTestId("condition-row").first();
  await row.waitFor();
  await sleep(250);
  // The field defaults to `verdict`, the only field of reviewer.review.
  if (film) {
    await choose(ctx, row.getByTestId("op-dropdown"), "neq");
    await choose(ctx, row.getByTestId("value-dropdown"), "pass");
  } else {
    await row.getByTestId("op-dropdown").selectOption("neq");
    await row.getByTestId("value-dropdown").selectOption("pass");
  }
  await sleep(400);
}

/** The reviewer → End edge becomes the default branch, off camera; then the
 *  loop edge is selected again, so the frames before the final click show the
 *  edge the viewer just watched. */
async function exitAsElse(ctx) {
  const { page } = ctx;
  const quick = { duration: 150, pause: 40 };
  const panel = page.getByTestId("edge-detail-panel");
  await ctx.click(await pointOnEdge(page, "e-2", 0.6), quick);
  // The panel re-renders for the new edge: never click the previous edge's toggle.
  await panel.filter({ hasText: /\.review\s*End/ }).getByTestId("else-toggle").waitFor();
  await sleep(250);
  const toggle = panel.getByTestId("else-toggle");
  if ((await toggle.getAttribute("aria-checked")) !== "true") await ctx.click(toggle, quick);
  await panel.getByTestId("else-active-note").waitFor({ timeout: 5_000 });
  await ctx.click(await pointOnEdge(page, "e-3", 0.55), quick);
  await panel.filter({ hasText: /\.review\s*implementer/ }).getByTestId("condition-row").waitFor();
  await sleep(300);
}

async function save(ctx) {
  await ctx.page.keyboard.press("Control+s");
  await ctx.page.getByText("Saved just now").waitFor({ timeout: 10_000 });
  await sleep(300);
}

export default {
  name: "routing",
  title: "Conditional routing & loops",
  needs: [],
  live: [],
  variants: [
    {
      id: "a",
      label: "Loop edge reviewer → implementer, condition verdict != pass (zoomed on the loop)",
      viewport: { width: 1100, height: 760 },
      localStorage: { "pdo.layout.run": { left: 17, center: 52, right: 31 } },
      // The canvas' full width, from above the loop region to below its exit.
      crop: { x: 188, y: 150, width: 570, height: 470 },
      markers: ["edge-dropped", "condition-saved"],
      async play(ctx) {
        const { page } = ctx;
        await restoreDemoPipeline(ctx.instance);
        await openPipeline(ctx);
        // Off camera: the camera zooms on implementer → reviewer.
        const a = await page.getByTestId("rf__node-implementer").boundingBox();
        const b = await page.getByTestId("rf__node-reviewer").boundingBox();
        await zoomCanvas(ctx, { x: a.x + a.width / 2 + 30, y: (a.y + b.y + b.height) / 2 + 25 });
        await ctx.moveTo(await emptyCanvasSpot(page), { duration: 300 });
        await ctx.keep(async () => {
          await sleep(400);
          await dragLoopEdge(ctx);
        });
        ctx.mark("edge-dropped", { before: 200, after: 900 });
        // Click the new edge (filmed), then its condition — the inspector is out of frame.
        await ctx.keep(async () => {
          await ctx.click(await pointOnEdge(page, "e-3", 0.55), { duration: 650 });
          await page.getByTestId("edge-detail-panel").waitFor();
          await sleep(500);
        });
        await setCondition(ctx, { film: false });
        await exitAsElse(ctx);
        await save(ctx);
        await ctx.keep(async () => {
          await ctx.click(await emptyCanvasSpot(page), { duration: 600 });
          await sleep(500);
        });
        ctx.mark("condition-saved", { before: 1300, after: 300 });
        await ctx.hold(2200);
      },
    },
    {
      id: "b",
      label: "The whole gesture, canvas and edge inspector at 1:1",
      viewport: { width: 1000, height: 760 },
      localStorage: { "pdo.layout.run": { left: 20, center: 44, right: 36 } },
      crop: { x: 201, y: 44, width: 799, height: 656 },
      markers: ["edge-dropped", "condition-saved"],
      async play(ctx) {
        const { page } = ctx;
        await restoreDemoPipeline(ctx.instance);
        await openPipeline(ctx);
        await ctx.keep(async () => {
          await sleep(400);
          await dragLoopEdge(ctx);
        });
        ctx.mark("edge-dropped", { before: 200, after: 600 });
        await ctx.keep(async () => {
          await ctx.click(await pointOnEdge(page, "e-3", 0.55), { duration: 650 });
          await page.getByTestId("edge-detail-panel").waitFor();
          await sleep(400);
          await setCondition(ctx, { film: true });
        });
        await exitAsElse(ctx);
        await save(ctx);
        await ctx.keep(async () => {
          await ctx.click(await emptyCanvasSpot(page), { duration: 700 });
          await sleep(500);
        });
        ctx.mark("condition-saved", { before: 1300, after: 300 });
        await ctx.hold(2200);
      },
    },
  ],
};
