// Scene « review » (row « Diff review »): the Review page of a run of the demo
// pipeline that REAL agents played to the end (`setup`, off camera, one run per
// variant: each films a review with no earlier comment). A line of its diff is
// commented, the comment is sent to the run's manager — a real
// `claude` session PDO starts on demand — and the manager's own answer
// (`pdo review reply`) lands in the thread. Nothing is injected: the scene
// waits for the reply the agent writes.
//
//   comment-posted → sent-to-manager → cut, ×8 while the manager reads → answer-received
//
//   a — `app.js`: the draft, then « Send all to manager » from the send bar.
//   b — `index.html`: the draft, then its own « Send ».
//
// Both in the unified view, the file list closed: the diff gets the whole
// window, so no code line, toolbar or thread footer is cut or wrapped at the
// GIF's width. The manager is stopped as soon as each variant is filmed
// (`stopDemoRun`).

import { sleep } from "../lib/demo-instance.mjs";
import { completeDemoRun, stopDemoRun } from "./_live.mjs";

/** Barely scaled down to the GIF's 960 px, nothing cropped: at this width the
 *  top bar holds its buttons on one line and the run label still reads. */
const VIEWPORT = { width: 1040, height: 660 };
/** Unified view, file list closed (both remembered by the browser). */
const LAYOUT = { "pdo.review.view": "unified", "pdo.review.list": "closed" };
/** Where the commented line sits on screen: the editor, the comment and the
 *  answer unfold below it. */
const LINE_TOP = 150;

/** The finished run each variant comments, by variant id. */
const runs = {};

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
async function openReview(ctx, runId, { file, line, text, fallback }) {
  const { page } = ctx;
  await ctx.goto(`/runs/${encodeURIComponent(runId)}/review`);
  const card = page.getByTestId("review-file").filter({ has: page.getByTestId("review-file-header").filter({ hasText: file }) }).first();
  await card.waitFor({ timeout: 30_000 });
  if ((await page.getByTestId("review-view-toggle").getAttribute("data-view")) !== "unified") throw new Error("the review is not in the unified view");
  // One table, added lines only.
  const rows = card.locator("tr").filter({ has: page.locator('[data-operator="+"]') });
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

/** The sent comment whose text is `comment` — the one this variant posted. */
function commentCard(page, comment) {
  return page.getByTestId("review-comment").filter({ has: page.getByTestId("review-comment-body").filter({ hasText: comment.slice(0, 40) }) }).first();
}

/** Film the wait for the manager: a short ×8 stretch, then cut until its
 *  answer (the reply its agent posted) is in THIS comment's thread. */
async function waitForAnswer(ctx, comment) {
  const { page } = ctx;
  const card = commentCard(page, comment);
  const answered = card.getByTestId("review-reply-body").first().waitFor({ timeout: 10 * 60_000 });
  await ctx.fast(() => Promise.race([answered, sleep(8_000)]), { speed: 8 });
  await answered;
  await sleep(500);
  // The whole thread in view (the answer can push past the window).
  const box = await card.boundingBox();
  if (box && box.y + box.height > VIEWPORT.height - 16) {
    await page.evaluate((dy) => {
      document.querySelector('[data-testid="review-main"]').scrollTop += dy;
    }, box.y + box.height - (VIEWPORT.height - 16));
    await sleep(400);
  }
}

/** The cursor rests on the comment's text, off every button and line number
 *  (a hovered one would stay lit on the poster). */
async function rest(ctx, comment) {
  const body = await commentCard(ctx.page, comment).getByTestId("review-comment-body").boundingBox();
  await ctx.hover({ x: body.x + body.width * 0.8, y: body.y + body.height / 2 }, { duration: 500, pause: 100 });
}

export default {
  name: "review",
  title: "Diff review — a comment sent to the manager, and its answer",
  live: ["claude"],
  async setup(instance) {
    // One real run per variant, off camera and side by side: each variant
    // comments a diff no earlier comment (or reply) sits on.
    const settled = await Promise.allSettled(["a", "b"].map(() => completeDemoRun(instance)));
    const failed = settled.find((s) => s.status === "rejected");
    if (failed) throw failed.reason;
    ["a", "b"].forEach((id, i) => {
      runs[id] = settled[i].value;
      console.log(`   demo run ${runs[id]} completed (variant ${id})`);
    });
  },
  variants: [
    {
      id: "a",
      label: "Comment a line of app.js, « Send all to manager », the manager's answer",
      viewport: VIEWPORT,
      localStorage: LAYOUT,
      markers: ["comment-posted", "sent-to-manager", "answer-received"],
      async play(ctx) {
        const { page, instance } = ctx;
        const runId = runs.a;
        if (!runId) throw new Error("no finished demo run (setup failed)");
        try {
          const { row, comment } = await openReview(ctx, runId, TARGETS.a);
          await ctx.keep(() => writeDraft(ctx, row, comment));
          ctx.mark("comment-posted", { before: 0, after: 700 });
          await sleep(700);
          await ctx.keep(async () => {
            await ctx.click(page.getByTestId("review-send-all"), { duration: 700 });
            await sent(page);
            await rest(ctx, comment);
          });
          ctx.mark("sent-to-manager", { before: 0, after: 1000 });
          await sleep(1000);
          await waitForAnswer(ctx, comment);
          await ctx.keep(() => rest(ctx, comment));
          ctx.mark("answer-received", { before: 0, after: 0 });
          await ctx.hold(2300);
        } finally {
          await stopDemoRun(instance, runId);
        }
      },
    },
    {
      id: "b",
      label: "Comment a line of index.html, send the draft on its own, the manager's answer",
      viewport: VIEWPORT,
      localStorage: LAYOUT,
      markers: ["comment-posted", "sent-to-manager", "answer-received"],
      async play(ctx) {
        const { page, instance } = ctx;
        const runId = runs.b;
        if (!runId) throw new Error("no finished demo run (setup failed)");
        try {
          const { row, comment } = await openReview(ctx, runId, TARGETS.b);
          await ctx.keep(() => writeDraft(ctx, row, comment));
          ctx.mark("comment-posted", { before: 0, after: 700 });
          await sleep(700);
          await ctx.keep(async () => {
            await ctx.click(page.getByTestId("review-comment-send").first(), { duration: 700 });
            await sent(page);
            await rest(ctx, comment);
          });
          ctx.mark("sent-to-manager", { before: 0, after: 1000 });
          await sleep(1000);
          await waitForAnswer(ctx, comment);
          await ctx.keep(() => rest(ctx, comment));
          ctx.mark("answer-received", { before: 0, after: 0 });
          await ctx.hold(2300);
        } finally {
          await stopDemoRun(instance, runId);
        }
      },
    },
  ],
};
