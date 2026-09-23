// Scene « outputs » (row « Typed outputs »): the two typed outputs of the demo
// pipeline's `reviewer`, on a run REAL agents played to the end (`setup`, off
// camera, one run for both variants).
//
//   Edit — its two declared ports: `review` (Markdown, with its `verdict` enum)
//          and `screenshots` (Image List), expected content « Screenshots of
//          the app, annotated with Pillow. »          → ports-declared
//   Run  — the node is done, so only its outputs show: an annotated screenshot
//          opens in the lightbox                        → image-opened
//          then the `review` markdown, its Mermaid diagram rendered → mermaid-rendered
//
//   a — the Edit tab scrolled to the Outputs, one screenshot, the diagram.
//   b — the image_list's expected content opened in full, two screenshots
//       (the lightbox's next arrow), the diagram.
//
// The mocked history prices the models: the node's cost reads in dollars.

import { sleep } from "../lib/demo-instance.mjs";
import { completeDemoRun, openRun, scrollUntil, DEMO_TASK } from "./_live.mjs";

const VIEWPORT = { width: 1100, height: 660 };
/** The inspector widened to 46 %: its port cards read without truncation. */
const LAYOUT = { "pdo.layout.run": { left: 16, center: 38, right: 46 } };
/** The whole window: the rail (the finished run selected), the canvas
 *  (`reviewer` selected) and the inspector. Not narrower: the lightbox spans
 *  nearly the whole window, top to bottom. */
const CROP = { x: 0, y: 0, width: 1100, height: 660 };

let runId = null;

/** Off camera: the finished run open, `reviewer` selected on its Run tab. */
async function openReviewer(ctx) {
  const { page } = ctx;
  await openRun(ctx, DEMO_TASK.name);
  await ctx.click(page.getByTestId("rf__node-reviewer"), { duration: 150, pause: 40 });
  await page.getByTestId("inspector-pane-run").getByTestId("image-thumbnails").waitFor({ timeout: 30_000 });
  // The thumbnails are loaded before anything is filmed.
  await page.waitForFunction(() => [...document.querySelectorAll('[data-testid^="thumbnail-"]')].every((img) => img.complete && img.naturalWidth > 0));
  await sleep(500);
}

/** Filmed: the Edit tab, scrolled down to the reviewer's two output ports:
 *  `review`'s card under the tab bar, or (`focus: "screenshots"`) the Image
 *  List card mid-panel, room left below it for its expected content. */
async function showDeclaredPorts(ctx, { focus = "review" } = {}) {
  const { page } = ctx;
  await ctx.click(page.getByTestId("inspector-tab-edit"), { duration: 700 });
  const screenshots = page.getByTestId("output-port-card-screenshots");
  await screenshots.waitFor({ timeout: 10_000 });
  await sleep(300);
  const target = page.getByTestId(`output-port-card-${focus}`);
  await scrollUntil(ctx, target, { over: page.getByTestId("inspector-pane-edit"), top: focus === "review" ? 96 : 330 });
  return screenshots;
}

/** Filmed: back to the Run tab, where the finished node shows its outputs. */
async function backToRun(ctx) {
  const { page } = ctx;
  await ctx.click(page.getByTestId("inspector-tab-run"), { duration: 700 });
  await page.getByTestId("inspector-pane-run").getByTestId("image-thumbnails").waitFor({ timeout: 10_000 });
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
  await ctx.keep(() => ctx.click(row, { duration: 700 }));
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

/** Reading the Edit tab edits nothing: the poster never shows « unsaved ». */
async function assertNothingUnsaved(page) {
  if (!(await page.getByTestId("save-button").isDisabled())) throw new Error("the run tab shows unsaved changes on the poster");
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
        await ctx.keep(() => showDeclaredPorts(ctx));
        const screenshots = page.getByTestId("output-port-card-screenshots");
        await ctx.hover(screenshots.getByText("Expected content"), { duration: 600, pause: 100 });
        ctx.mark("ports-declared", { before: 300, after: 1100 });
        await sleep(1100);
        await ctx.keep(async () => {
          await backToRun(ctx);
          await openScreenshot(ctx, 0);
        });
        ctx.mark("image-opened", { before: 200, after: 1300 });
        await sleep(1300);
        await ctx.keep(async () => {
          await ctx.click(page.getByTestId("lightbox-close"), { duration: 550 });
          await sleep(300);
        });
        await openReviewMarkdown(ctx);
        ctx.mark("mermaid-rendered", { before: 200, after: 0 });
        await ctx.hold(2200);
        await assertNothingUnsaved(page);
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
        const screenshots = await ctx.keep(() => showDeclaredPorts(ctx, { focus: "screenshots" }));
        await ctx.keep(async () => {
          // Opening the expected content reads it; nothing is edited, the Save stays off.
          await ctx.click(screenshots.getByText("Expected content"), { duration: 600 });
          await sleep(500);
        });
        ctx.mark("ports-declared", { before: 0, after: 1100 });
        await sleep(1100);
        await ctx.keep(async () => {
          await backToRun(ctx);
          await openScreenshot(ctx, 0);
        });
        ctx.mark("image-opened", { before: 200, after: 800 });
        await sleep(800);
        await ctx.keep(async () => {
          await ctx.click(page.getByTestId("lightbox-next"), { duration: 550 });
          await sleep(1000);
        });
        // Cut: the lightbox closes off camera, the next shot opens on the outputs.
        await ctx.click(page.getByTestId("lightbox-close"), { duration: 150, pause: 40 });
        await sleep(400);
        await openReviewMarkdown(ctx);
        ctx.mark("mermaid-rendered", { before: 200, after: 0 });
        await ctx.hold(2200);
        await assertNothingUnsaved(page);
      },
    },
  ],
};
