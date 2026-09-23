// Scene « Triggers » (README row 5, #857, #883). No live agent: the demo
// trigger `prod-health-check` fires `prod-check` (its own target,
// fixture/targets/prod-check.yaml: Incident-debugger → Orchestrate Fix →
// Notify Slack, and straight to End on a false alarm) every minute
// (`* * * * *`) behind the guard `./prod-health-check.sh`, a script of the
// fixture repo `~/code/shop-app`. The guard exits 0 when prod looks degraded,
// and its stdout (the incident report) becomes the run's input
// (`{{guard_stdout}}`). `prod-check` never runs live: the fire history is the
// mocked one (lib/history-plan.mjs), about a hundred fires, about one in ten a
// real incident.
//
//   a — Test guard (dry-run: exit 0 + the incident report), then the fire
//       history (the published one).
//   b — The runs the trigger fired, from the runs rail filtered on it: false
//       alarms (« Alert: checkout p95 spike »), and among them a real incident.
//
// The trigger is shown ARMED (no « disabled » badge) while it can never fire:
// the global Trigger pause is on before it is enabled, so the scheduler skips
// every tick. The pause banner lives in the Triggers tab of the left panel,
// which variant a crops out and variant b never films (it films the Runs tab).

import { sleep } from "../lib/demo-instance.mjs";
import { PROD_CHECK } from "../lib/history-plan.mjs";
import { DEMO_TRIGGER } from "../lib/seed.mjs";
import { assertNoAgent, forbidAgentLaunch, waitForRuns } from "./_no-agent.mjs";

const VIEWPORT = { width: 1100, height: 700 };
// Canvas (the pipeline the trigger fires) + the trigger panel, wide enough for
// the guard's report to read in full; the top bar, the left panel (and its
// pause banner) and the status bar are cut.
const LAYOUT = { "pdo.layout.run": { left: 18, center: 36, right: 46 } };
const CROP = { x: 198, y: 44, width: 902, height: 634 };
// Variant b: the runs rail, the canvas and the top of the trigger panel.
const VIEWPORT_B = { width: 1040, height: 680 };
const LAYOUT_B = { "pdo.layout.run": { left: 21, center: 37, right: 42 } };
const CROP_B = { x: 0, y: 44, width: 1040, height: 614 };
/** How far the canvas is panned right, off camera: the drawn `Incident_found =
 *  false` label sits left of the nodes, past the edge of the fitted view. */
const PAN_X = 70;

/** From the runs list to the trigger's detail panel and its canvas, off camera. */
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
  await panel.getByTestId("fire-entry").first().waitFor({ timeout: 30_000 });
  // The repo reads `~/code/shop-app` once the daemon's home is known.
  await panel.getByTestId("trigger-detail-repo").getByText("~/code/shop-app", { exact: true }).waitFor({ timeout: 60_000 });
  for (const node of PROD_CHECK.nodeDefs.filter((n) => n.node_type === "agent")) {
    await page.locator(".react-flow__node").filter({ hasText: node.name }).first().waitFor();
  }
  await panCanvas(page, PAN_X);
  await sleep(500);
  return panel;
}

/** Pan the edit canvas by `dx`, dragging its empty bottom-left corner. Off
 *  camera; the view is not part of the pipeline, so nothing turns unsaved. */
async function panCanvas(page, dx) {
  const box = await page.getByTestId("rf__wrapper").boundingBox();
  const y = box.y + box.height - 30;
  await page.mouse.move(box.x + 24, y);
  await page.mouse.down();
  await page.mouse.move(box.x + 24 + dx / 2, y, { steps: 6 });
  await page.mouse.move(box.x + 24 + dx, y, { steps: 6 });
  await page.mouse.up();
  await sleep(300);
  const falseLabel = page.getByText("Incident_found = false", { exact: true }).first();
  const label = await falseLabel.boundingBox();
  if (!label || label.x < box.x + 4) throw new Error(`the « Incident_found = false » label is cut by the canvas edge (${JSON.stringify(label)})`);
}

/** The poster never shows an unsaved pipeline. */
async function assertSaved(page) {
  const save = page.getByRole("button", { name: "Save", exact: true });
  if (!(await save.isDisabled())) throw new Error("the prod-check canvas reads unsaved");
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
    if (trigger.pipeline_id !== PROD_CHECK.id) throw new Error(`the demo trigger fires ${trigger.pipeline_id}, not ${PROD_CHECK.id}`);
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
        // Down the history: fire after fire, each the run it started.
        const entries = panel.getByTestId("fire-entry");
        const shown = entries.nth(3);
        await ctx.keep(async () => {
          await scrollTo(ctx, panel, shown, 0.95);
          await ctx.hover(shown.getByTestId("fire-run-link"), { duration: 600, pause: 300 });
        });
        ctx.mark("history-shown", { before: 200, after: 600 });
        // Off the link (its hover state) before the end state, the poster.
        const box = await shown.boundingBox();
        await ctx.keep(() => ctx.moveTo({ x: box.x + box.width * 0.62, y: box.y + box.height + 26 }, { duration: 500 }));
        await assertSaved(page);
        await ctx.hold(1600);
        assertNoAgent(ctx.instance);
      },
    },
    {
      id: "b",
      label: "The runs it fired: false alarms, and a real incident",
      viewport: VIEWPORT_B,
      crop: CROP_B,
      localStorage: LAYOUT_B,
      markers: ["runs-filtered", "incident-shown"],
      async play(ctx) {
        const { page } = ctx;
        const panel = await openTrigger(ctx);
        // The Runs tab, off camera (the Triggers tab holds the pause banner);
        // the trigger panel stays on the right.
        await page.getByTestId("left-tab-runs").click();
        await waitForRuns(page);
        await sleep(400);
        await ctx.hold(700);
        await ctx.keep(async () => {
          await ctx.hover(panel.getByTestId("trigger-detail-schedule"), { duration: 700, pause: 350 });
          await ctx.hover(panel.getByTestId("trigger-detail-guard"), { duration: 450, pause: 350 });
          await ctx.click(page.getByTestId("run-filter-trigger"), { duration: 700 });
          const option = page.locator('[data-testid^="run-filter-option-"]').filter({ hasText: DEMO_TRIGGER.name }).first();
          await option.waitFor();
          await sleep(200);
          await ctx.click(option, { duration: 450 });
        });
        const labels = page.getByTestId("run-display-label");
        await labels.filter({ hasText: "Alert: checkout p95 spike" }).first().waitFor();
        await sleep(300);
        const shownNames = await labels.evaluateAll((els) =>
          els.filter((e) => e.getBoundingClientRect().bottom < window.innerHeight).map((e) => e.textContent ?? ""),
        );
        if (shownNames.some((n) => !/^(Alert|Incident):/.test(n))) throw new Error(`the rail should list only the trigger's runs, got ${shownNames.join(" | ")}`);
        ctx.mark("runs-filtered", { before: 200, after: 900 });
        // The first real incident among the false alarms, in view.
        const incident = labels.filter({ hasText: "Incident: checkout API degraded" }).first();
        const rail = await labels.first().boundingBox();
        await ctx.keep(async () => {
          await ctx.moveTo({ x: rail.x + rail.width * 0.6, y: VIEWPORT_B.height * 0.55 }, { duration: 500 });
          const box = await incident.boundingBox();
          const bottom = VIEWPORT_B.height * 0.8;
          if (box.y + box.height > bottom) await ctx.scroll(box.y + box.height - bottom, { duration: 800 });
          await sleep(250);
          await ctx.hover(incident, { duration: 600, pause: 300 });
        });
        ctx.mark("incident-shown", { before: 200, after: 700 });
        // Off the row (its hover actions) before the poster.
        const box = await incident.boundingBox();
        await ctx.keep(() => ctx.moveTo({ x: box.x + box.width + 150, y: box.y + box.height / 2 }, { duration: 500 }));
        await assertSaved(page);
        await ctx.hold(1600);
        assertNoAgent(ctx.instance);
      },
    },
  ],
};
