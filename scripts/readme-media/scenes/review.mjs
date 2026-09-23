// Scene « review » (row « Diff review »): the Review page of a run of the demo
// pipeline that REAL agents played to the end (`setup`, off camera). A line of
// its diff is commented, the comment is sent to the run's manager — a real
// `claude` session PDO starts on demand — and the manager's own answer
// (`pdo review reply`) lands in the thread. Nothing is injected: the scene
// waits for the reply the agent writes.
//
//   comment-posted → sent-to-manager → cut, ×8 while the manager reads → answer-received
//
//   a — split view, `app.js`: the draft, then « Send all to manager » from the send bar.
//   b — unified view, `index.html`: the draft, then its own « Send ».
//
// The manager is stopped as soon as each variant is filmed (`stopDemoRun`);
// variant b's send starts a fresh one.

import { sleep } from "../lib/demo-instance.mjs";
import { completeDemoRun, stopDemoRun } from "./_live.mjs";

const VIEWPORT = { width: 1180, height: 680 };
/** The diff column, header included: the file list on the left (and the run
 *  id at the top left) is cut. */
const CROP = { x: 272, y: 0, width: 908, height: 680 };
/** Where the commented line sits on screen: the editor, the comment and the
 *  answer unfold below it, inside the crop. */
const LINE_TOP = 150;

let runId = null;

/** The comment of each variant, on a line the demo task always adds (with a
 *  generic fallback on the file's first added line). */
const TARGETS = {
  a: {
    file: "app.js",
    line: /addEventListener\(\s*["']input["']/,
    text: "Filtering on every keystroke re-renders the whole list. Worth a small debounce here?",
    fallback: "Could you add a one-line comment saying why this is needed?",
  },
  b: {
    file: "index.html",
    line: /<input[^>]*(type="search"|id="search")/,
    text: 'Should this input get autocomplete="off", so the browser\'s suggestions don\'t cover the results?',
    fallback: "Could you add a one-line comment saying why this is needed?",
  },
};

/** Off camera: the run's Review page, the target line of `file` scrolled to
 *  `LINE_TOP`. Returns the row and the comment text that fits it. */
async function openReview(ctx, { file, line, text, fallback }, { unified = false } = {}) {
  const { page } = ctx;
  await ctx.goto(`/runs/${encodeURIComponent(runId)}/review`);
  const card = page.getByTestId("review-file").filter({ has: page.getByTestId("review-file-header").filter({ hasText: file }) }).first();
  await card.waitFor({ timeout: 30_000 });
  if (unified) {
    await ctx.click(page.getByTestId("review-view-unified"), { duration: 150, pause: 40 });
    await sleep(600);
  }
  // Split: the destination (new) side. Unified: one table, added lines only.
  const rows = unified ? card.locator("tr").filter({ has: page.locator('[data-operator="+"]') }) : card.locator('table[data-mode="new"] tr').filter({ has: page.locator('[data-operator="+"]') });
  await rows.first().waitFor({ timeout: 30_000 });
  let row = rows.filter({ hasText: line }).first();
  let comment = text;
  if ((await row.count()) === 0) {
    row = rows.first();
    comment = fallback;
  }
  await row.evaluate((el, top) => {
    const main = document.querySelector('[data-testid="review-main"]');
    main.scrollTop += el.getBoundingClientRect().top - top;
  }, LINE_TOP);
  await sleep(700);
  return { row, comment };
}

/** Filmed: hover the line number, press its « + », type the comment, save the draft. */
async function writeDraft(ctx, row, comment) {
  const { page } = ctx;
  const number = row.locator("td").first();
  await ctx.hover(number, { duration: 800, pause: 250 });
  await ctx.click(row.locator("button.diff-add-widget").first(), { duration: 250 });
  const editor = page.getByTestId("review-editor-text");
  await editor.waitFor({ timeout: 5_000 });
  await sleep(250);
  await page.keyboard.type(comment, { delay: 22 });
  await sleep(300);
  await ctx.click(page.getByTestId("review-editor-save"), { duration: 600 });
  await page.getByTestId("review-send-bar").waitFor({ timeout: 5_000 });
  await sleep(300);
}

/** The send landed: no draft is left, so the send bar is gone. */
async function sent(page) {
  await page.getByTestId("review-send-bar").waitFor({ state: "detached", timeout: 30_000 });
  await sleep(300);
}

/** Film the wait for the manager: a short ×8 stretch, then cut until its
 *  answer (the reply its agent posted) is in the thread. */
async function waitForAnswer(ctx) {
  const { page } = ctx;
  const reply = page.getByTestId("review-reply").first();
  const answered = reply.waitFor({ timeout: 10 * 60_000 });
  await ctx.fast(() => Promise.race([answered, sleep(12_000)]), { speed: 8 });
  await answered;
  await page.getByTestId("review-reply-body").first().waitFor();
  await sleep(500);
  // The whole thread in view (the answer can push past the crop).
  const box = await page.getByTestId("review-comment-thread").first().boundingBox();
  if (box && box.y + box.height > CROP.height - 16) {
    await page.evaluate((dy) => {
      document.querySelector('[data-testid="review-main"]').scrollTop += dy;
    }, box.y + box.height - (CROP.height - 16));
    await sleep(400);
  }
}

/** The cursor rests on the comment's text, off every button and line number
 *  (a hovered one would stay lit on the poster). */
async function rest(ctx) {
  const body = await ctx.page.getByTestId("review-comment-body").first().boundingBox();
  await ctx.hover({ x: body.x + body.width * 0.8, y: body.y + body.height / 2 }, { duration: 500, pause: 100 });
}

export default {
  name: "review",
  title: "Diff review — a comment sent to the manager, and its answer",
  live: ["claude"],
  async setup(instance) {
    // One real run, off camera: its diff is what both variants comment.
    runId = await completeDemoRun(instance);
    console.log(`   demo run ${runId} completed`);
  },
  variants: [
    {
      id: "a",
      label: "Split view: comment a line of app.js, « Send all to manager », the manager's answer",
      viewport: VIEWPORT,
      crop: CROP,
      markers: ["comment-posted", "sent-to-manager", "answer-received"],
      async play(ctx) {
        const { page, instance } = ctx;
        if (!runId) throw new Error("no finished demo run (setup failed)");
        try {
          const { row, comment } = await openReview(ctx, TARGETS.a);
          await ctx.keep(() => writeDraft(ctx, row, comment));
          ctx.mark("comment-posted", { before: 0, after: 700 });
          await sleep(700);
          await ctx.keep(async () => {
            await ctx.click(page.getByTestId("review-send-all"), { duration: 700 });
            await sent(page);
            await rest(ctx);
          });
          ctx.mark("sent-to-manager", { before: 0, after: 1000 });
          await sleep(1000);
          await waitForAnswer(ctx);
          await ctx.keep(() => rest(ctx));
          ctx.mark("answer-received", { before: 0, after: 0 });
          await ctx.hold(2300);
        } finally {
          await stopDemoRun(instance, runId);
        }
      },
    },
    {
      id: "b",
      label: "Unified view: comment a line of index.html, send the draft, the manager's answer",
      viewport: VIEWPORT,
      crop: CROP,
      markers: ["comment-posted", "sent-to-manager", "answer-received"],
      async play(ctx) {
        const { page, instance } = ctx;
        if (!runId) throw new Error("no finished demo run (setup failed)");
        try {
          const { row, comment } = await openReview(ctx, TARGETS.b, { unified: true });
          await ctx.keep(() => writeDraft(ctx, row, comment));
          ctx.mark("comment-posted", { before: 0, after: 700 });
          await sleep(700);
          await ctx.keep(async () => {
            await ctx.click(page.getByTestId("review-comment-send").first(), { duration: 700 });
            await sent(page);
            await rest(ctx);
          });
          ctx.mark("sent-to-manager", { before: 0, after: 1000 });
          await sleep(1000);
          await waitForAnswer(ctx);
          await ctx.keep(() => rest(ctx));
          ctx.mark("answer-received", { before: 0, after: 0 });
          await ctx.hold(2300);
        } finally {
          await stopDemoRun(instance, runId);
        }
      },
    },
  ],
};
