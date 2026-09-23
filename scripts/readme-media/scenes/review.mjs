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
// Both zoomed on the commented hunk and its thread (the comment, the send, the
// manager's answer): the unified view, the file list closed, a narrow window
// cropped under the Review toolbar to ~720 px, scaled up to the GIF's 960. The
// toolbar and the file list are out of frame; the send bar (sticky at the
// bottom) is in, and the crop ends with it. The manager answers in one line
// (`MANAGER_RULE`, the demo HOME's `~/.claude/CLAUDE.md`, written once the demo
// runs are done). It is stopped as soon as each variant is filmed
// (`stopDemoRun`).

import fs from "node:fs";
import path from "node:path";
import { sleep } from "../lib/demo-instance.mjs";
import { completeDemoRun, stopDemoRun } from "./_live.mjs";

/** A narrow window: the diff's file cards and the thread (720 px at most) fill
 *  its width, and the send bar ends inside the crop. Its height stops the
 *  frame after the commented hunk: the next file stays out of the poster. */
const VIEWPORT = { width: 760, height: 430 };
/** The Review toolbar's height (`grid-rows-[36px_1fr]`): the crop starts under it. */
const TOOLBAR = 36;
/** The send bar is sticky 8 px above the page's bottom (`bottom-2`): under it,
 *  the next file's rows scroll by, half cut. */
const SEND_BAR_GAP = 8;
/** The hunk and its thread, under the toolbar, from the file card's left edge
 *  to the send bar's right end, down to the send bar's bottom: ~720 px scaled
 *  up to 960. */
const CROP = { x: 24, y: TOOLBAR + 8, width: 720, height: VIEWPORT.height - SEND_BAR_GAP - TOOLBAR - 8 };
/** Unified view, file list closed (both remembered by the browser). */
const LAYOUT = { "pdo.review.view": "unified", "pdo.review.list": "closed" };
/** Where the commented line sits on screen: a few lines of its hunk above it,
 *  the editor, the comment and the answer unfolding below it. */
const LINE_TOP = CROP.y + 74;

/** What the manager reads, in the demo HOME's `~/.claude/CLAUDE.md`: its
 *  answer fits on one line of the thread (~100 characters at the crop's width). */
export const MANAGER_RULE = [
  "# Review answers",
  "",
  "When you answer a review comment with `pdo review reply`, answer in ONE short sentence of at most 80 characters,",
  'for example: --text "Done in 1a2b3c4: debounced at 150 ms, the list re-renders once typing pauses."',
  "",
].join("\n");

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
    text: 'Could this input get aria-controls="products", for screen readers?',
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
  const main = await page.getByTestId("review-main").boundingBox();
  if (main.y > CROP.y) throw new Error(`the diff starts at ${main.y} px, under the crop's top (${CROP.y} px): the toolbar would show`);
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

/** Type `text` a word at a time: a key per character re-renders the editor on
 *  each stroke and drags the typing over seconds of GIF. */
async function typeWords(page, text) {
  for (const word of text.match(/\S+\s*/g)) {
    await page.keyboard.insertText(word);
    await sleep(70);
  }
}

/** Filmed: hover the line number, press its « + », type the comment, save the
 *  draft. Then, off camera, the « Draft saved » toast goes (it would sit over
 *  the send bar): the next kept frame shows the draft and the send bar alone. */
async function writeDraft(ctx, row, comment) {
  const { page } = ctx;
  await ctx.keep(async () => {
    const number = row.locator("td").first();
    await ctx.hover(number, { duration: 800, pause: 250 });
    await ctx.click(row.locator("button.diff-add-widget").first(), { duration: 250 });
    const editor = page.getByTestId("review-editor-text");
    await editor.waitFor({ timeout: 5_000 });
    await sleep(250);
    await typeWords(page, comment);
    await sleep(300);
    await ctx.click(page.getByTestId("review-editor-save"), { duration: 600 });
  });
  await page.getByTestId("review-send-bar").waitFor({ timeout: 5_000 });
  await page.getByTestId("review-toast").waitFor({ state: "detached", timeout: 10_000 });
  await assertSendBarEndsCrop(page);
  await sleep(200);
}

/** The crop ends with the send bar: nothing of the next file shows under it. */
async function assertSendBarEndsCrop(page) {
  const bar = await page.getByTestId("review-send-bar").boundingBox();
  const bottom = CROP.y + CROP.height;
  if (!bar || Math.abs(bar.y + bar.height - bottom) > 2) {
    throw new Error(`the send bar ends at ${bar && bar.y + bar.height} px, not at the crop's bottom (${bottom} px)`);
  }
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
  await ctx.fast(() => Promise.race([answered, sleep(6_000)]), { speed: 8 });
  await answered;
  await sleep(500);
  // The whole thread in the crop (the answer can push past its bottom).
  const box = await card.boundingBox();
  const bottom = CROP.y + CROP.height - 16;
  if (box && box.y + box.height > bottom) {
    await page.evaluate((dy) => {
      document.querySelector('[data-testid="review-main"]').scrollTop += dy;
    }, box.y + box.height - bottom);
    await sleep(400);
  }
  await assertOneLineAnswer(card);
}

/** The manager's answer holds on one line of the thread: no wrap. */
async function assertOneLineAnswer(card) {
  // One client rect per line box of the text (margins do not count).
  const lines = await card.getByTestId("review-reply-body").first().evaluate((el) => {
    const range = document.createRange();
    range.selectNodeContents(el);
    // A code span's box sits a few px off its line's: rects within 6 px share a line.
    const bottoms = [...range.getClientRects()].filter((r) => r.width > 0).map((r) => r.bottom).sort((x, y) => x - y);
    return bottoms.filter((b, i) => i === 0 || b - bottoms[i - 1] > 6).length;
  });
  if (lines > 1) throw new Error(`the manager's answer wraps over ${lines} lines of the thread`);
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
    // Only the manager, started on the first send, reads it: the runs are done.
    const memory = path.join(instance.home, ".claude", "CLAUDE.md");
    fs.mkdirSync(path.dirname(memory), { recursive: true });
    fs.writeFileSync(memory, MANAGER_RULE);
  },
  variants: [
    {
      id: "a",
      label: "Comment a line of app.js, « Send all to manager », the manager's answer",
      viewport: VIEWPORT,
      localStorage: LAYOUT,
      crop: CROP,
      markers: ["comment-posted", "sent-to-manager", "answer-received"],
      async play(ctx) {
        const { page, instance } = ctx;
        const runId = runs.a;
        if (!runId) throw new Error("no finished demo run (setup failed)");
        try {
          const { row, comment } = await openReview(ctx, runId, TARGETS.a);
          await writeDraft(ctx, row, comment);
          ctx.mark("comment-posted", { before: 0, after: 500 });
          await sleep(500);
          await ctx.keep(async () => {
            await ctx.click(page.getByTestId("review-send-all"), { duration: 700 });
            await sent(page);
            await rest(ctx, comment);
          });
          ctx.mark("sent-to-manager", { before: 0, after: 800 });
          await sleep(800);
          await waitForAnswer(ctx, comment);
          await ctx.keep(() => rest(ctx, comment));
          ctx.mark("answer-received", { before: 0, after: 0 });
          await ctx.hold(2000);
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
      crop: CROP,
      markers: ["comment-posted", "sent-to-manager", "answer-received"],
      async play(ctx) {
        const { page, instance } = ctx;
        const runId = runs.b;
        if (!runId) throw new Error("no finished demo run (setup failed)");
        try {
          const { row, comment } = await openReview(ctx, runId, TARGETS.b);
          await writeDraft(ctx, row, comment);
          ctx.mark("comment-posted", { before: 0, after: 500 });
          await sleep(500);
          await ctx.keep(async () => {
            await ctx.click(page.getByTestId("review-comment-send").first(), { duration: 700 });
            await sent(page);
            await rest(ctx, comment);
          });
          ctx.mark("sent-to-manager", { before: 0, after: 800 });
          await sleep(800);
          await waitForAnswer(ctx, comment);
          await ctx.keep(() => rest(ctx, comment));
          ctx.mark("answer-received", { before: 0, after: 0 });
          await ctx.hold(2000);
        } finally {
          await stopDemoRun(instance, runId);
        }
      },
    },
  ],
};
