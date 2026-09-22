// Scene « Run stats by model » (README row 6). No live agent: the Stats page
// reads the mocked history of the demo pipeline `implement-review` (#856) —
// Opus 5.5, Fable 5.1 (low / high), GPT-5.6 Sol and GLM-5.3 Flash on its
// `implementer` and `reviewer` nodes, over the last 30 days.
//
//   a — Cost › By model, then a model → its efforts (the published one).
//   b — Performance › By model: the context and duration box-plots.

import { sleep } from "../lib/demo-instance.mjs";

const VIEWPORT = { width: 1200, height: 760 };
// Narrower window for variant a: the modal reflows, so the crop keeps the
// ranked list, the headline, the chart and the effort rows at ~1:1 in the GIF.
const VIEWPORT_A = { width: 1080, height: 760 };

/** From the runs list to the Stats modal, on `tab`. `film` keeps the two
 *  clicks in the GIF (only when the crop shows the top bar and the rail). */
async function openStats(ctx, tab, { film: filmed = true } = {}) {
  const { page } = ctx;
  await ctx.goto("/");
  await page.getByTestId("open-stats").waitFor();
  // The rail fills a beat after the page: never open on « No runs yet ».
  await page.getByTestId("run-display-label").first().waitFor({ timeout: 30_000 });
  await sleep(400);
  const film = (fn) => (filmed ? ctx.keep(fn) : fn());
  await film(async () => {
    await ctx.click(page.getByTestId("open-stats"), { duration: 800 });
    await page.getByTestId("stats-modal").waitFor();
  });
  // The Overview's first load is cut out of the GIF.
  await page.getByTestId("stats-chart-runs").waitFor({ timeout: 60_000 });
  await sleep(300);
  await film(() => ctx.click(page.getByTestId(`stats-tab-${tab}`), { duration: 700 }));
  // The heavy read (transcripts × prices) is cut out of the GIF.
  await page.getByTestId(tab === "cost" ? "stats-selection-headline" : "stats-performance-headline").waitFor({ timeout: 60_000 });
  await sleep(800);
}

/** Pick a value in a native <select>: the cursor clicks it, the page picks. */
async function choose(ctx, select, value) {
  await ctx.click(select, { duration: 650 });
  await sleep(250);
  await select.selectOption(value);
  await ctx.page.keyboard.press("Escape").catch(() => {});
}

export default {
  name: "stats",
  title: "Run stats by model",
  needs: ["history"],
  live: [],
  variants: [
    {
      id: "a",
      label: "Cost › By model → effort",
      viewport: VIEWPORT_A,
      // Focused on the Cost panel: the top bar and the Stats sections are cut.
      crop: { x: 140, y: 150, width: 940, height: 610 },
      markers: ["cost-by-model", "effort"],
      async play(ctx) {
        const { page } = ctx;
        await openStats(ctx, "cost", { film: false });
        const nav = page.getByTestId("stats-drilldown-navigation");
        await ctx.keep(async () => {
          await choose(ctx, page.getByLabel("Cost grouping"), "model");
          await nav.getByText("claude-opus-5-5").waitFor();
          await sleep(300);
          ctx.mark("cost-by-model", { before: 300, after: 1200 });
          for (const model of ["claude-opus-5-5", "claude-fable-5-1", "gpt-5.6-sol", "z-ai/glm-5.3-flash"]) {
            await ctx.hover(nav.getByText(model, { exact: true }), { duration: 320, pause: 260 });
          }
          await ctx.click(nav.getByText("claude-fable-5-1", { exact: true }), { duration: 600 });
          await page.getByTestId("stats-selection-headline").getByText(/median per execution/).waitFor();
          await sleep(500);
        });
        ctx.mark("effort", { before: 200, after: 800 });
        const detail = page.getByTestId("stats-drilldown-detail");
        await ctx.keep(async () => {
          // Point at the figure, not the name: the name's provenance tooltip
          // would stay on the poster.
          const box = await detail.getByTestId("stats-detail-row").first().boundingBox();
          await ctx.hover({ x: box.x + box.width * 0.62, y: box.y + box.height / 2 }, { duration: 650, pause: 500 });
        });
        await ctx.hold(1600);
      },
    },
    {
      id: "b",
      label: "Performance › By model, box-plots",
      viewport: VIEWPORT,
      crop: { x: 150, y: 150, width: 1050, height: 610 },
      markers: ["performance", "by-model"],
      async play(ctx) {
        const { page } = ctx;
        await openStats(ctx, "performance", { film: false });
        await ctx.hover(page.getByTestId("stats-performance-headline"), { duration: 500, pause: 200 });
        ctx.mark("performance", { before: 1500, after: 600 });
        await ctx.keep(async () => {
          await choose(ctx, page.getByLabel("Performance grouping"), "model");
          await sleep(900);
        });
        ctx.mark("by-model", { before: 200, after: 600 });
        // Down to the distributions: one box-plot row per model.
        const rows = page.getByTestId("stats-detail-row");
        await ctx.keep(async () => {
          await ctx.moveTo(page.getByTestId("stats-performance-headline"), { duration: 450 });
          const top = (await rows.first().boundingBox())?.y ?? 700;
          await ctx.scroll(Math.max(0, top - 330), { duration: 900 });
          await sleep(400);
          const count = Math.min(await rows.count(), 4);
          // Each model row has one line per harness (claude, copilot, pi):
          // point at the line that has a box-plot, not at « never ran on … ».
          const harnessLine = [0.2, 0.2, 0.5, 0.8];
          for (let i = 0; i < count; i++) {
            const box = await rows.nth(i).boundingBox();
            await ctx.hover({ x: box.x + box.width * 0.36, y: box.y + box.height * harnessLine[i] }, { duration: 420, pause: 420 });
          }
          // Off every tooltip before the end state (the poster).
          const last = await rows.nth(count - 1).boundingBox();
          await ctx.moveTo({ x: last.x - 110, y: last.y + last.height / 2 }, { duration: 500 });
        });
        await ctx.hold(1800);
      },
    },
  ],
};
