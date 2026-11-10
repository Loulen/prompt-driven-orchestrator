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
//   b — the same build with Pipeline info open in the inspector (its node and
//       edge counts follow).

import { sleep } from "../lib/demo-instance.mjs";
import { targetPipeline } from "../lib/targets.mjs";
import { clearToolbar, drawEdge, dragNodeTo, emptyCanvasSpot, installStartState, landOnTarget, nudgeNodeTo, openPipeline, saveCanvas } from "./_canvas.mjs";

const TARGET = "implement-review.pipelines";
/** What the gestures draw: `implementer` (its two edges go with it). */
const DRAWN = { nodes: ["implementer"] };

// Filmed at the 100 % UI scale (Settings › Interface; the front's default is
// 120 %), in a 1280 px window: the + menu drops a new card in the first FREE
// slot of the visible canvas, clear of Start and End by a card and a gap, and
// only a canvas this wide (with the inspector open) has one on screen.
const VIEWPORT = { width: 1280, height: 680 };
const LAYOUT = { "pdo.ui.scale": "100", "pdo.layout.run": { left: 16, center: 59, right: 25 } };
/** Under the tab bar (the « Draft » line shows from the first change on), down
 *  to the status bar: a — the canvas between the left panel and the
 *  inspector; b — the canvas and the inspector. */
const CROP_A = { x: 206, y: 68, width: 753, height: 590 };
const CROP_B = { x: 206, y: 68, width: 1074, height: 590 };

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
  // Deselect: the empty inspector folds (variant b opens Pipeline info again).
  await ctx.click(await emptyCanvasSpot(ctx.page), { duration: 150, pause: 40 });
  await sleep(200);
}

/** A target edge, its `implementer` end renamed to the id the editor gave the node. */
function withNodeId(edge, id) {
  const end = (e) => (e.node === "implementer" ? { ...e, node: id } : e);
  return { ...edge, source: end(edge.source), target: end(edge.target) };
}

/** Pipeline info in the inspector, off camera: the toolbar's ⓘ (a toggle). */
async function openPipelineInfo(ctx) {
  if (await ctx.page.getByTestId("info-panel-name").isVisible()) return;
  await ctx.click(ctx.page.getByTestId("toolbar-info"), { duration: 150, pause: 40 });
  await ctx.page.getByTestId("info-panel-name").waitFor();
  await sleep(300);
}

async function play(ctx, { markNode, markEdges }) {
  const { page } = ctx;
  const target = targetPipeline(TARGET);
  await installStartState(ctx.instance, TARGET, DRAWN);
  // No zoom: the editor drops a new card in the first free slot of the VISIBLE
  // canvas, and zoomed in there is none — it would land out of frame.
  await openPipeline(ctx);
  // The inspector open from the start, on Pipeline info: an empty one folds
  // (#949), and a node added or selected unfolds it, which would resize the
  // canvas mid-take. Variant a crops it out, variant b films its counts.
  await openPipelineInfo(ctx);
  await clearToolbar(ctx);
  await ctx.moveTo(await emptyCanvasSpot(page), { duration: 200 });
  const id = await ctx.keep(async () => {
    await sleep(150);
    return addImplementer(ctx);
  });
  await shareRunWorktree(ctx, id);
  // Selecting the node swapped Pipeline info for its inspector: back to it.
  await openPipelineInfo(ctx);
  // The cut back lands on the new card, never on the toolbar (its tooltips).
  await ctx.moveTo(page.getByTestId(`rf__node-${id}`), { duration: 150 });
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
      crop: CROP_A,
      markers: ["node-added", "edge-dropped"],
      play: (ctx) => play(ctx, { markNode: { before: 200, after: 300 }, markEdges: { before: 200, after: 300 } }),
    },
    {
      id: "b",
      label: "The same build, with Pipeline info in the inspector (its counts follow)",
      viewport: VIEWPORT,
      localStorage: LAYOUT,
      crop: CROP_B,
      markers: ["node-added", "edge-dropped"],
      play: (ctx) => play(ctx, { markNode: { before: 200, after: 500 }, markEdges: { before: 200, after: 300 } }),
    },
  ],
};
