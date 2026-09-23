// Scene « Visual pipelines » (README row 1). No live agent: the demo pipeline
// `implement-review` is built on the edit canvas. Each variant starts from its
// skeleton (Start and End only, under the demo name `implement-review`),
// drops an `implementer` node from the toolbar's + menu, then drags its edges.
//
//   a — (the published one) add `implementer`, drag Start → implementer, then
//       implementer → End: a connected graph. Focused on the canvas.
//   b — add `implementer`, drag its edge to End, with the Pipeline Inspector in
//       frame (its node and edge counts follow).

import { sleep } from "../lib/demo-instance.mjs";
import { emptyCanvasSpot, handle, installPipeline, openPipeline } from "./_canvas.mjs";

/** The demo pipeline before it is built: its entry and its end. */
const SKELETON = `name: implement-review
version: '1.0'
nodes:
- id: start
  name: Start
  type: start
  outputs:
  - name: user_prompt
    side: bottom
  view:
    x: 300
    y: 0
- id: end
  name: End
  type: end
  inputs:
  - name: result
    side: top
  view:
    x: 300
    y: 300
edges: []
`;

const VIEWPORT = { width: 1040, height: 760 };
const LAYOUT = { "pdo.layout.run": { left: 19, center: 57, right: 24 } };

/** + → Node: the new agent node is named `implementer` by default. */
async function addImplementer(ctx) {
  const { page } = ctx;
  await ctx.click(page.getByTestId("toolbar-add"), { duration: 800 });
  await page.getByTestId("add-menu-node").waitFor();
  await sleep(350);
  await ctx.click(page.getByTestId("add-menu-node"), { duration: 450 });
  const node = page.locator(".react-flow__node").filter({ hasText: "implementer" }).first();
  await node.waitFor();
  await sleep(500);
  return node;
}

/** Drag from a port's handle and drop on a node's card. */
async function dragEdge(ctx, fromHandle, toNode) {
  await ctx.drag(fromHandle, toNode, { duration: 1000 });
  await sleep(600);
}

async function endState(ctx) {
  await ctx.moveTo(await emptyCanvasSpot(ctx.page), { duration: 700 });
  await ctx.hold(2000);
}

export default {
  name: "pipelines",
  title: "Visual pipelines",
  needs: [],
  live: [],
  variants: [
    {
      id: "a",
      label: "Add implementer, wire Start → implementer → End",
      viewport: VIEWPORT,
      localStorage: LAYOUT,
      // The canvas, its tab and its toolbar (where the + menu opens).
      crop: { x: 198, y: 44, width: 593, height: 560 },
      markers: ["node-added", "edge-dropped"],
      async play(ctx) {
        const { page } = ctx;
        await installPipeline(ctx.instance, SKELETON, { nodes: 2, edges: 0 });
        await openPipeline(ctx);
        let implementer;
        await ctx.keep(async () => {
          await sleep(500);
          implementer = await addImplementer(ctx);
        });
        ctx.mark("node-added", { before: 200, after: 300 });
        await ctx.keep(async () => {
          await dragEdge(ctx, handle(page.getByTestId("rf__node-start"), "user_prompt"), implementer);
          await dragEdge(ctx, handle(implementer, "out"), page.getByTestId("rf__node-end"));
        });
        ctx.mark("edge-dropped", { before: 200, after: 300 });
        await endState(ctx);
      },
    },
    {
      id: "b",
      label: "Add implementer, drag its edge to End (with the Pipeline Inspector)",
      viewport: VIEWPORT,
      localStorage: LAYOUT,
      crop: { x: 198, y: 44, width: 842, height: 560 },
      markers: ["node-added", "edge-dropped"],
      async play(ctx) {
        const { page } = ctx;
        await installPipeline(ctx.instance, SKELETON, { nodes: 2, edges: 0 });
        await openPipeline(ctx);
        let implementer;
        await ctx.keep(async () => {
          await sleep(500);
          implementer = await addImplementer(ctx);
        });
        ctx.mark("node-added", { before: 200, after: 500 });
        await ctx.keep(() => dragEdge(ctx, handle(implementer, "out"), page.getByTestId("rf__node-end")));
        ctx.mark("edge-dropped", { before: 200, after: 300 });
        await endState(ctx);
      },
    },
  ],
};
