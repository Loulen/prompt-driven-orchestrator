// Scene « hero » (top of the README): a REAL run of the demo pipeline
// `implement-review` on the fixture shop — Claude Code (`claude` /
// `claude-opus-5-5`, named by the terminal's status line) at work in the
// `implementer` node's terminal, then the `reviewer`'s typed outputs.
//
//   a — the run plays, the camera zooms on `implementer`, and Claude Code works
//       in the terminal next to it. Ends on the live terminal; its run is
//       stopped at once (it never waits for the reviewer).
//   b — (the published one, design « Hero: Variant B ») the run plays, click
//       `implementer` → live terminal (×8) → cut → click `reviewer` → its typed
//       outputs: the verdict and the `image_list` of annotated screenshots.
//       Big, full window, no camera zoom: 1280×760 with the inspector widened to
//       43 % (`pdo.layout.run`), the final frame on the `image_list`. The
//       manifest notes the choice (label: « … ends on the reviewer's outputs »).
//
// The mocked history (`needs: ["history"]`) fills the runs rail and prices the
// models, so the rail is never empty and the cost reads in dollars.

import { sleep } from "../lib/demo-instance.mjs";
import { DEMO_TASK, nodeState, openRun, startDemoRun, stopDemoRun, waitNode, waitPane } from "./_live.mjs";

const VIEWPORT = { width: 1280, height: 760 };
const LAYOUT = { "pdo.layout.run": { left: 15, center: 42, right: 43 } };
/** Claude Code is up: its status line names the model (lib/credentials.mjs). */
const CLAUDE_UP = /Opus 5\.5 · claude-opus-5-5/;
/** A variant A run of its own, so the two runs never share a rail row. */
const TASK_A = {
  name: "Add a price sort",
  input:
    "Add a select above the product list that sorts the products by price (low to high, high to low). Remember the choice in localStorage, and document the feature in the README.",
};

/** Select `Start` off camera: its inspector shows the run's input (the task).
 *  Opening a run selects its live node by itself; this puts the inspector back
 *  on a neutral page before a filmed click, so the click visibly opens a node. */
async function selectStart(ctx) {
  await ctx.click(ctx.page.getByTestId("rf__node-start"), { duration: 150, pause: 40 });
  await ctx.page.getByText("Run start").first().waitFor({ timeout: 10_000 });
  await sleep(500);
}

/** Click `implementer` on the run canvas (filmed), then wait off camera until
 *  its terminal is attached and Claude Code is working in it. */
async function openImplementerTerminal(ctx, runId) {
  const { page, instance } = ctx;
  await ctx.keep(() => ctx.click(page.getByTestId("rf__node-implementer"), { duration: 750 }));
  await page.getByText("attached · live").waitFor({ timeout: 30_000 });
  await waitPane(instance, runId, "implementer", CLAUDE_UP);
  await sleep(400);
}

/** Film the implementer working (fast-forwarded) until it completes, and stop
 *  the fast stretch before the pane turns into « [exited] ». */
async function fastUntilDone(ctx, runId, { speed = 8, cap = 90_000 } = {}) {
  const { instance } = ctx;
  await ctx.fast(
    async () => {
      const deadline = Date.now() + cap;
      while (Date.now() < deadline) {
        const node = await nodeState(instance, runId, "implementer");
        if (node && node.status !== "running") return;
        await sleep(120);
      }
    },
    { speed },
  );
}

export default {
  name: "hero",
  title: "Hero — a run with Claude Code at work",
  needs: ["history"],
  live: ["claude"],
  variants: [
    {
      id: "a",
      label: "Run plays, zoom on implementer, Claude Code at work in the terminal",
      viewport: VIEWPORT,
      localStorage: LAYOUT,
      // The canvas and the inspector — `implementer` next to its terminal — from
      // the tab bar down to the node's outputs (the runs rail is cut).
      crop: { x: 193, y: 44, width: 1087, height: 646 },
      gifWidth: 1200,
      markers: ["run-started", "zoom", "terminal-active"],
      async play(ctx) {
        const { page, instance } = ctx;
        let runId;
        try {
          runId = await startDemoRun(instance, TASK_A);
          await openRun(ctx, TASK_A.name);
          await selectStart(ctx);
          ctx.mark("run-started", { before: 0, after: 900 });
          const implementer = page.getByTestId("rf__node-implementer");
          await ctx.keep(async () => {
            await sleep(600);
            // The camera zooms on the node: the wheel zooms the canvas around the cursor.
            await ctx.moveTo(implementer, { duration: 700 });
            await ctx.scroll(-240, { duration: 900 });
            await sleep(300);
          });
          ctx.mark("zoom", { before: 1300, after: 300 });
          await openImplementerTerminal(ctx, runId);
          ctx.mark("terminal-active", { before: 0, after: 2400 });
          await sleep(2400);
          // The poster is the terminal still at work: never wait for the end.
          await ctx.fast(() => sleep(3200), { speed: 8 });
          await ctx.hold(1800);
          const { status } = await nodeState(instance, runId, "implementer");
          if (status !== "running") throw new Error(`implementer already ${status}: the poster would show an exited terminal`);
        } finally {
          await stopDemoRun(instance, runId, { archive: true });
        }
      },
    },
    {
      id: "b",
      label: "Run plays, live terminal ×8, cut, ends on the reviewer's typed outputs (verdict + image_list)",
      viewport: VIEWPORT,
      localStorage: LAYOUT,
      gifWidth: 1280,
      markers: ["run-started", "terminal-active", "output-ready"],
      async play(ctx) {
        const { page, instance } = ctx;
        let runId;
        try {
          runId = await startDemoRun(instance, DEMO_TASK);
          await openRun(ctx, DEMO_TASK.name);
          await selectStart(ctx);
          // Opens on a run already moving: `implementer` running on the canvas.
          ctx.mark("run-started", { before: 0, after: 900 });
          await openImplementerTerminal(ctx, runId);
          ctx.mark("terminal-active", { before: 0, after: 2200 });
          await sleep(2200);
          await fastUntilDone(ctx, runId);

          // Cut: the reviewer's work (Playwright + Pillow) is not filmed.
          const reviewer = await waitNode(instance, runId, "reviewer");
          if (reviewer.status !== "completed") throw new Error(`reviewer ended ${reviewer.status}`);
          await waitNode(instance, runId, "end", { statuses: ["completed"], timeout: 60_000 }).catch(() => {});
          await selectStart(ctx);
          await sleep(600);

          const pane = page.getByTestId("inspector-pane-run");
          const outputs = pane.getByTestId("details-pane");
          await ctx.keep(async () => {
            await ctx.click(page.getByTestId("rf__node-reviewer"), { duration: 800 });
            await pane.getByTestId("image-thumbnails").waitFor({ timeout: 30_000 });
            await sleep(700);
            // Down to the `image_list` (a no-op when it already shows).
            await ctx.moveTo(outputs, { duration: 600 });
            await ctx.scroll(600, { duration: 700 });
            await sleep(300);
          });
          // Beside the thumbnails, not on one: a hovered thumbnail would stay lit on the poster.
          const thumbs = await pane.getByTestId("image-thumbnails").boundingBox();
          await ctx.hover({ x: thumbs.x + thumbs.width + 40, y: thumbs.y + thumbs.height / 2 }, { duration: 700, pause: 300 });
          ctx.mark("output-ready", { before: 1400, after: 400 });
          await ctx.hold(2600);
        } finally {
          await stopDemoRun(instance, runId);
        }
      },
    },
  ],
};
