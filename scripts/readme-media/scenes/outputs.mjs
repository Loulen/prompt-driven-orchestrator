// Scene « outputs » (row « Typed outputs »): the two typed outputs of the demo
// pipeline's `reviewer`, on a run REAL agents played to the end (`setup`, off
// camera, one run for both variants).
//
//   Edit — its two declared ports, in the pipeline's editor (a run is
//          read-only and shows no ports): `review` (Markdown, with its `verdict` enum)
//          and `screenshots` (Image List), expected content « Screenshots of
//          the app, annotated with Pillow. »          → ports-declared
//   Run  — the node is done: its terminal folds to a one-line bar (#346) and
//          the panel is its I/O — the one input `code`, then the two outputs.
//          An annotated screenshot opens in the lightbox → image-opened
//          then the `review` markdown, its Mermaid diagram rendered → mermaid-rendered
//
//   a — the editor scrolled to the Outputs, one screenshot, the diagram.
//   b — the image_list's expected content opened in full, two screenshots
//       (the lightbox's next arrow), the diagram.
//
// The tab switches and the scroll down to the ports play ×3 (NAV_SPEED): the
// row stays in 8 to 15 s. The mocked history prices the models: the node's
// cost reads in dollars.

import { sleep } from "../lib/demo-instance.mjs";
import { DEMO_PIPELINE_ID } from "../lib/targets.mjs";
import { completeDemoRun, openRun, scrollUntil, DEMO_TASK } from "./_live.mjs";

const VIEWPORT = { width: 1100, height: 660 };
/** The inspector widened to 46 %: its port cards read without truncation. */
const LAYOUT = { "pdo.layout.run": { left: 16, center: 38, right: 46 } };
/** The tab switches and the scroll to the ports play at this speed: they
 *  move the panel, not the story. */
const NAV_SPEED = 3;

/** The whole window: the rail (the finished run selected), the canvas
 *  (`reviewer` selected) and the inspector. Not narrower: the lightbox spans
 *  nearly the whole window, top to bottom. */
const CROP = { x: 0, y: 0, width: 1100, height: 660 };

let runId = null;

const quick = { duration: 150, pause: 40 };
/** The canvas zoomed out one step, off camera: the loop's pill, right of its
 *  cards, fits in the narrow canvas beside the wide inspector. */
async function zoomOutOnce(ctx) {
  await ctx.click(ctx.page.getByTestId("toolbar-zoom-out").locator("visible=true"), quick);
  await sleep(400);
}

/** Off camera, two tabs: the finished run, `reviewer` selected on its Run tab
 *  (its outputs loaded), then the demo pipeline's editor, `reviewer` selected
 *  (its declared ports). The take opens on the editor. */
async function openReviewer(ctx) {
  const { page } = ctx;
  await openRun(ctx, DEMO_TASK.name);
  await zoomOutOnce(ctx);
  const thumbnails = page.getByTestId("inspector-pane-run").getByTestId("image-thumbnails");
  // A click while the canvas still settles can miss the node (Pipeline info
  // opens instead): try again, off camera.
  for (let attempt = 1; ; attempt++) {
    await ctx.click(page.getByTestId("rf__node-reviewer").locator("visible=true"), quick);
    try {
      await thumbnails.waitFor({ timeout: 8_000 });
      break;
    } catch (error) {
      if (attempt === 3) throw error;
      await sleep(800);
    }
  }
  // The thumbnails are loaded before anything is filmed.
  await page.waitForFunction(() => [...document.querySelectorAll('[data-testid^="thumbnail-"]')].every((img) => img.complete && img.naturalWidth > 0));
  // The pipeline the run was started from, in its editor: a run is read-only
  // (ADR-0085) and its Config tab shows no ports; the editor declares them.
  await ctx.click(page.getByTestId("left-tab-library"), quick);
  await ctx.click(page.getByTestId(`library-row-${DEMO_PIPELINE_ID}`), quick);
  // The editor's canvas; its inspector, empty, stays folded until reviewer is selected.
  await page.getByTestId("validate-button").waitFor();
  await sleep(600);
  await zoomOutOnce(ctx);
  await ctx.click(page.getByTestId("rf__node-reviewer").locator("visible=true"), quick);
  await page.getByTestId("output-port-card-screenshots").waitFor({ timeout: 10_000 });
  await sleep(500);
}

/** Filmed: the editor's inspector, scrolled down to the reviewer's two output
 *  ports: `review`'s card near the top, or (`focus: "screenshots"`) the Image
 *  List card mid-panel, room left below it for its expected content. */
async function showDeclaredPorts(ctx, { focus = "review" } = {}) {
  const { page } = ctx;
  const screenshots = page.getByTestId("output-port-card-screenshots");
  const target = page.getByTestId(`output-port-card-${focus}`);
  await scrollUntil(ctx, target, { over: page.getByTestId("inspector-pane-edit"), top: focus === "review" ? 96 : 330 });
  return screenshots;
}

/** Filmed: back to the run, from the runs rail (the narrow tab bar folds its
 *  tab into its overflow menu), where the finished node shows its outputs. */
async function backToRun(ctx) {
  const { page } = ctx;
  await ctx.click(page.getByTestId("left-tab-runs"), { duration: 600 });
  const row = page.getByTestId("run-display-label").filter({ hasText: DEMO_TASK.name }).first();
  await row.waitFor();
  await ctx.click(row, { duration: 600 });
  const thumbnails = page.getByTestId("inspector-pane-run").getByTestId("image-thumbnails");
  // The run tab comes back on its last selection; if it lost it, reviewer is clicked again.
  if (!(await thumbnails.waitFor({ timeout: 3_000 }).then(() => true, () => false))) {
    await ctx.click(page.getByTestId("rf__node-reviewer").locator("visible=true"), { duration: 600 });
  }
  await thumbnails.waitFor({ timeout: 10_000 });
  await sleep(300);
}

/** Filmed: an annotated screenshot in the lightbox (loaded before the marker). */
async function openScreenshot(ctx, index = 0) {
  const { page } = ctx;
  await ctx.click(page.getByTestId(`thumbnail-${index}`), { duration: 700 });
  const image = page.getByTestId("lightbox-image");
  await image.waitFor({ timeout: 10_000 });
  await image.evaluate((img) => (img.complete ? null : new Promise((resolve) => img.addEventListener("load", resolve, { once: true }))));
  await sleep(300);
}

/** The `review` markdown: the click is filmed, the Mermaid render is waited
 *  for off camera (cut), then the diagram is shown in full. */
async function openReviewMarkdown(ctx) {
  const { page } = ctx;
  const row = page.getByTestId("inspector-pane-run").locator('[data-testid="port-row"][data-port="review"][data-kind="output"]');
  await ctx.keep(() => ctx.click(row, { duration: 550 }));
  const dialog = page.locator('[role="dialog"][data-port="review"]');
  const diagram = dialog.getByTestId("mermaid-diagram");
  await diagram.locator("svg").first().waitFor({ timeout: 30_000 });
  await sleep(300);
  await ctx.keep(async () => {
    const box = await diagram.boundingBox();
    // The diagram sits under the verdict: scroll only if it is not in full view.
    if (box.y + box.height > CROP.y + CROP.height - 40) {
      await scrollUntil(ctx, diagram, { over: dialog, top: Math.max(CROP.y + 120, CROP.y + CROP.height - 40 - box.height) });
    }
    // The cursor rests beside the modal, not on the diagram.
    const modal = await dialog.boundingBox();
    await ctx.hover({ x: modal.x + modal.width + 60, y: modal.y + modal.height * 0.6 }, { duration: 500, pause: 150 });
  });
}

/** Reading the editor's ports edits nothing: checked on the editor, before the
 *  take goes back to the run (read-only, no Save button). */
async function assertNothingUnsaved(page) {
  if (!(await page.getByTestId("save-button").isDisabled())) throw new Error("reading the reviewer's ports left the pipeline unsaved");
}

export default {
  name: "outputs",
  title: "Typed outputs — the reviewer's ports, then its outputs",
  needs: ["history"],
  live: ["claude"],
  async setup(instance) {
    // One real run of the demo pipeline, off camera: its reviewer's outputs are
    // what both variants open. Its agents have exited when this returns.
    runId = await completeDemoRun(instance);
    console.log(`   demo run ${runId} completed`);
  },
  variants: [
    {
      id: "a",
      label: "Edit: the two declared ports, Run: an annotated screenshot, the Mermaid diagram",
      viewport: VIEWPORT,
      localStorage: LAYOUT,
      crop: CROP,
      markers: ["ports-declared", "image-opened", "mermaid-rendered"],
      async play(ctx) {
        const { page } = ctx;
        if (!runId) throw new Error("no finished demo run (setup failed)");
        await openReviewer(ctx);
        await ctx.fast(() => showDeclaredPorts(ctx), { speed: NAV_SPEED });
        const screenshots = page.getByTestId("output-port-card-screenshots");
        await ctx.hover(screenshots.getByText("Expected content"), { duration: 600, pause: 100 });
        ctx.mark("ports-declared", { before: 700, after: 1000 });
        await sleep(1000);
        await assertNothingUnsaved(page);
        await ctx.fast(() => backToRun(ctx), { speed: NAV_SPEED });
        await ctx.keep(() => openScreenshot(ctx, 0));
        ctx.mark("image-opened", { before: 200, after: 1200 });
        await sleep(1200);
        await ctx.keep(async () => {
          await ctx.click(page.getByTestId("lightbox-close"), { duration: 550 });
          await sleep(300);
        });
        await openReviewMarkdown(ctx);
        ctx.mark("mermaid-rendered", { before: 200, after: 0 });
        await ctx.hold(1900);
      },
    },
    {
      id: "b",
      label: "Edit: the image_list's expected content in full, Run: two annotated screenshots, the Mermaid diagram",
      viewport: VIEWPORT,
      localStorage: LAYOUT,
      crop: CROP,
      markers: ["ports-declared", "image-opened", "mermaid-rendered"],
      async play(ctx) {
        const { page } = ctx;
        if (!runId) throw new Error("no finished demo run (setup failed)");
        await openReviewer(ctx);
        const screenshots = await ctx.fast(() => showDeclaredPorts(ctx, { focus: "screenshots" }), { speed: NAV_SPEED });
        await ctx.keep(async () => {
          // Opening the expected content reads it; nothing is edited, the Save stays off.
          await ctx.click(screenshots.getByText("Expected content"), { duration: 500 });
          await sleep(400);
        });
        ctx.mark("ports-declared", { before: 0, after: 1000 });
        await sleep(1000);
        await assertNothingUnsaved(page);
        await ctx.fast(() => backToRun(ctx), { speed: NAV_SPEED });
        await ctx.keep(() => openScreenshot(ctx, 0));
        ctx.mark("image-opened", { before: 200, after: 700 });
        await sleep(700);
        await ctx.keep(async () => {
          await ctx.click(page.getByTestId("lightbox-next"), { duration: 500 });
          await sleep(700);
        });
        // Cut: the lightbox closes off camera, the next shot opens on the outputs.
        await ctx.click(page.getByTestId("lightbox-close"), { duration: 150, pause: 40 });
        await sleep(400);
        await openReviewMarkdown(ctx);
        ctx.mark("mermaid-rendered", { before: 200, after: 0 });
        await ctx.hold(1900);
      },
    },
  ],
};
