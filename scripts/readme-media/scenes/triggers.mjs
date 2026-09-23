// Scene « Triggers » (README row 5, #857). No live agent: the demo trigger
// `prod-health-check` fires the demo pipeline `implement-review` every minute
// (`* * * * *`) behind the guard `./prod-health-check.sh`, a script of the
// fixture repo. The guard exits 0 when prod is degraded, and its stdout (the
// incident report) becomes the input of `implementer` (`{{guard_stdout}}`).
// The fire history is the mocked one (#856): mostly skipped, a few fired.
//
//   a — Test guard (dry-run: exit 0 + the incident report), then the fire
//       history down to a fired entry (the published one).
//   b — The same dry-run, then a skipped fire's guard output (prod healthy).
//
// The trigger is shown ARMED (no « disabled » badge) while it can never fire:
// the global Trigger pause is on before it is enabled, so the scheduler skips
// every tick. The pause banner lives in the left panel, which the crop leaves out.

import { sleep } from "../lib/demo-instance.mjs";
import { DEMO_TRIGGER } from "../lib/seed.mjs";
import { assertNoAgent, forbidAgentLaunch, waitForRuns } from "./_no-agent.mjs";

const VIEWPORT = { width: 1100, height: 700 };
// Canvas (the pipeline the trigger fires) + the trigger panel, wide enough for
// the guard's report to read in full; the top bar, the left panel (and its
// pause banner) and the status bar are cut.
const LAYOUT = { "pdo.layout.run": { left: 18, center: 36, right: 46 } };
const CROP = { x: 198, y: 44, width: 902, height: 634 };

/** From the runs list to the trigger's detail panel, off camera. */
async function openTrigger(ctx) {
  const { page } = ctx;
  await forbidAgentLaunch(page);
  await ctx.goto("/");
  await waitForRuns(page);
  await page.getByTestId("left-tab-triggers").click();
  // The name, never the row's centre: the row carries hover actions (Run now).
  await page.getByTestId("trigger-row").getByText(DEMO_TRIGGER.name, { exact: true }).click();
  const panel = page.getByTestId("trigger-detail-panel");
  await panel.getByTestId("fire-history").waitFor({ timeout: 30_000 });
  await page.locator(".react-flow__node").filter({ hasText: "reviewer" }).first().waitFor();
  await sleep(700);
  return panel;
}

/** Point at the cron and the guard, click Test guard, wait for the verdict. */
async function testGuard(ctx, panel) {
  await ctx.keep(async () => {
    await ctx.hover(panel.getByTestId("trigger-detail-schedule"), { duration: 700, pause: 450 });
    await ctx.hover(panel.getByTestId("trigger-detail-guard"), { duration: 450, pause: 400 });
    await ctx.click(panel.getByTestId("guard-test-button"), { duration: 450 });
  });
  // The guard's run is cut: the GIF goes from the click to its verdict.
  await panel.getByTestId("guard-test-result").waitFor({ timeout: 15_000 });
  await sleep(250);
  const verdict = await panel.getByTestId("guard-test-verdict").textContent();
  if (!/would fire/i.test(verdict ?? "")) throw new Error(`the guard dry-run should pass (exit 0), got « ${verdict} »`);
  ctx.mark("guard-tested", { before: 200, after: 1600 });
  // Along the report, line by line: it is the run's input.
  await ctx.keep(async () => {
    const output = panel.getByTestId("guard-test-output");
    const box = await output.boundingBox();
    await ctx.moveTo({ x: box.x + box.width * 0.72, y: box.y + box.height * 0.3 }, { duration: 500 });
    await ctx.moveTo({ x: box.x + box.width * 0.72, y: box.y + box.height * 0.85 }, { duration: 700 });
  });
}

/** Scroll the panel so the BOTTOM of `entry` sits at `at` of its height:
 *  as little as possible, so the guard's report stays in the frame. */
async function scrollTo(ctx, panel, entry, at) {
  const pane = await panel.boundingBox();
  const box = await entry.boundingBox();
  await ctx.moveTo({ x: pane.x + pane.width * 0.5, y: pane.y + pane.height * 0.6 }, { duration: 450 });
  await ctx.scroll(Math.max(0, box.y + box.height - (pane.y + pane.height * at)), { duration: 900 });
  await sleep(300);
}

export default {
  name: "triggers",
  title: "Triggers",
  needs: ["history"],
  live: [],
  async setup(instance) {
    // Pause first, then arm: at no moment can the every-minute cron fire.
    await instance.api("POST", "/triggers/pause", { paused: true });
    const triggers = await instance.api("GET", "/triggers");
    const trigger = triggers.find((t) => t.name === DEMO_TRIGGER.name);
    if (!trigger) throw new Error(`no demo trigger « ${DEMO_TRIGGER.name} » (is the history seeded?)`);
    await instance.api("PATCH", `/triggers/${trigger.id}`, { enabled: true });
  },
  variants: [
    {
      id: "a",
      label: "Guard dry-run, then the fire history",
      viewport: VIEWPORT,
      crop: CROP,
      localStorage: LAYOUT,
      markers: ["guard-tested", "history-shown"],
      async play(ctx) {
        const { page } = ctx;
        const panel = await openTrigger(ctx);
        await ctx.hold(700);
        await testGuard(ctx, panel);
        const fired = panel.getByTestId("fire-entry").filter({ has: page.getByTestId("fire-run-link") }).first();
        await ctx.keep(async () => {
          await scrollTo(ctx, panel, fired, 0.93);
          await ctx.hover(fired.getByTestId("fire-run-link"), { duration: 600, pause: 300 });
        });
        ctx.mark("history-shown", { before: 200, after: 600 });
        // Off the link (its hover state) before the end state, the poster.
        const box = await fired.boundingBox();
        await ctx.keep(() => ctx.moveTo({ x: box.x + box.width * 0.62, y: box.y + box.height + 26 }, { duration: 500 }));
        await ctx.hold(1600);
        assertNoAgent(ctx.instance);
      },
    },
    {
      id: "b",
      label: "Guard dry-run, then a skipped fire's guard output",
      viewport: VIEWPORT,
      crop: CROP,
      localStorage: LAYOUT,
      markers: ["guard-tested", "history-shown"],
      async play(ctx) {
        const { page } = ctx;
        const panel = await openTrigger(ctx);
        await ctx.hold(700);
        await testGuard(ctx, panel);
        const skipped = panel.getByTestId("fire-entry").filter({ has: page.getByTestId("fire-guard-output-toggle") }).first();
        await ctx.keep(async () => {
          await scrollTo(ctx, panel, skipped, 0.82);
          await ctx.click(skipped.getByTestId("fire-guard-output-toggle"), { duration: 600 });
          await skipped.getByTestId("fire-guard-output").waitFor();
          await sleep(200);
          // The opened output grows the entry: bring its end into the frame.
          await scrollTo(ctx, panel, skipped, 0.97);
        });
        ctx.mark("history-shown", { before: 200, after: 600 });
        const box = await skipped.boundingBox();
        await ctx.keep(() => ctx.moveTo({ x: box.x + box.width * 0.78, y: box.y + box.height - 14 }, { duration: 500 }));
        await ctx.hold(1600);
        assertNoAgent(ctx.instance);
      },
    },
  ],
};
