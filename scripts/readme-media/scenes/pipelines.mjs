// Scene « Visual pipelines » (README row 1). No live agent. The demo pipeline
// `implement-review` is built on the edit canvas, from its target
// (fixture/targets/implement-review.pipelines.yaml, #882): the take starts from
// the target without `implementer` (Start and End where the maintainer drew
// them), the cursor drops an `implementer` from the toolbar's + menu, drags it
// to its place, then wires Start → implementer and implementer → End from the
// cards' rims to the target's anchors (#840). Saved; the drift from the target
// goes to the manifest, and the target is reinstalled off camera: the poster is
// the maintainer's drawing.
//
//   a — (the published one) focused on the canvas.
//   b — the same build with the Pipeline Inspector in frame (its node and edge
//       counts follow).

import { sleep } from "../lib/demo-instance.mjs";
import { targetPipeline } from "../lib/targets.mjs";
import { clearToolbar, drawEdge, dragNodeTo, emptyCanvasSpot, installStartState, landOnTarget, nudgeNodeTo, openPipeline, saveCanvas } from "./_canvas.mjs";

const TARGET = "implement-review.pipelines";
/** What the gestures draw: `implementer` (its two edges go with it). */
const DRAWN = { nodes: ["implementer"] };

// The canvas is wide enough (~630 px) for the new card to land to the right of
// the Start–End column (the editor's first free slot), in frame.
const VIEWPORT = { width: 1100, height: 760 };
const LAYOUT = { "pdo.layout.run": { left: 18, center: 57, right: 25 } };

const edgeOf = (target, from, to) => target.edges.find((e) => e.source.node === from && e.target.node === to);

/** + → Node: the new agent node is named `implementer` by default. Returns
 *  the id the editor gave it. */
async function addImplementer(ctx) {
  const { page } = ctx;
  await ctx.click(page.getByTestId("toolbar-add"), { duration: 600 });
  await page.getByTestId("add-menu-node").waitFor();
  await sleep(250);
  await ctx.click(page.getByTestId("add-menu-node"), { duration: 400 });
  const node = page.locator(".react-flow__node").filter({ hasText: "implementer" }).first();
  await node.waitFor();
  return node.getAttribute("data-id");
}

/** The target's `implementer` shares the run worktree; a new agent node is
 *  isolated (its card shows the marker). Set off camera, in its node inspector. */
async function shareRunWorktree(ctx, id) {
  await ctx.click(ctx.page.getByTestId(`rf__node-${id}`), { duration: 150, pause: 40 });
  const shared = ctx.page.getByTestId("workspace-shared");
  await shared.waitFor();
  await ctx.click(shared, { duration: 150, pause: 40 });
  await ctx.page.getByTestId("isolation-marker").waitFor({ state: "detached", timeout: 5_000 });
  // Deselect: the inspector shows the pipeline again (its counts, variant b).
  await ctx.click(await emptyCanvasSpot(ctx.page), { duration: 150, pause: 40 });
  await sleep(200);
}

/** A target edge, its `implementer` end renamed to the id the editor gave the node. */
function withNodeId(edge, id) {
  const end = (e) => (e.node === "implementer" ? { ...e, node: id } : e);
  return { ...edge, source: end(edge.source), target: end(edge.target) };
}

async function play(ctx, { markNode, markEdges }) {
  const { page } = ctx;
  const target = targetPipeline(TARGET);
  await installStartState(ctx.instance, TARGET, DRAWN);
  // No zoom: the editor drops a new card in the first free slot of the VISIBLE
  // canvas, and zoomed in there is none — it would land out of frame.
  await openPipeline(ctx);
  await clearToolbar(ctx);
  await ctx.moveTo(await emptyCanvasSpot(page), { duration: 200 });
  const id = await ctx.keep(async () => {
    await sleep(150);
    return addImplementer(ctx);
  });
  const spot = { ...ctx.mouse };
  await shareRunWorktree(ctx, id);
  await ctx.moveTo(spot, { duration: 150 });
  await sleep(300);
  // The new card lands where the maintainer drew it.
  const view = target.nodes.find((n) => n.id === "implementer").view;
  await dragNodeTo(ctx, id, view);
  ctx.mark("node-added", markNode);
  await sleep(markNode.after + 100);
  await nudgeNodeTo(ctx, id, view);
  await drawEdge(ctx, withNodeId(edgeOf(target, "start", "implementer"), id));
  await drawEdge(ctx, withNodeId(edgeOf(target, "implementer", "end"), id));
  ctx.mark("edge-dropped", markEdges);
  // Filmed: the cursor leaves the frame, rather than vanish at the cut to the final shot.
  const rest = await emptyCanvasSpot(page);
  await ctx.keep(() => ctx.moveTo(rest, { duration: 400 }));
  await saveCanvas(ctx);
  await landOnTarget(ctx, TARGET);
  await ctx.hold(1800);
}

export default {
  name: "pipelines",
  title: "Visual pipelines",
  needs: [],
  live: [],
  drawn: DRAWN,
  variants: [
    {
      id: "a",
      label: "Drop implementer, wire Start → implementer → End (the target)",
      viewport: VIEWPORT,
      localStorage: LAYOUT,
      // The canvas and its toolbar (where the + menu opens), below the tab bar:
      // no « unsaved » dot in frame.
      crop: { x: 198, y: 76, width: 627, height: 528 },
      markers: ["node-added", "edge-dropped"],
      play: (ctx) => play(ctx, { markNode: { before: 200, after: 300 }, markEdges: { before: 200, after: 300 } }),
    },
    {
      id: "b",
      label: "The same build, with the Pipeline Inspector (its counts follow)",
      viewport: VIEWPORT,
      localStorage: LAYOUT,
      crop: { x: 198, y: 76, width: 902, height: 528 },
      markers: ["node-added", "edge-dropped"],
      play: (ctx) => play(ctx, { markNode: { before: 200, after: 500 }, markEdges: { before: 200, after: 300 } }),
    },
  ],
};
