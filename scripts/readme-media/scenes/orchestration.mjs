// Scene « orchestration » (row « Recursive orchestration »): the demo pipeline
// relaunching ITSELF. Its `implementer` runs with Orchestrator on (`setup`
// installs the target `implement-review` with the toggle on, every other byte
// as drawn) and gets a feature in two parts: from its own
// session, the REAL agent starts one child run of `implement-review` per part
// (`pdo run create`), then waits for them (`pdo run wait --all`). The children
// are REAL runs too — their own implementer and reviewer, on the fixture shop.
//
//   child-created — the children appear, nested under their parent in the run tree
//   counters      — the child counters move as the children finish
//
//   a — the whole window: the run tree on the left, `implementer`'s
//       Orchestration tab on the right (child rows, counters, cost). Its
//       terminal folds to a bar on the poster, once its session ended.
//   b — the run tree and the canvas: the parent row folded on its chevron, its
//       counters and the canvas node's counting the children to the end.
//
// Each variant plays its own run, and stops it with its children once filmed.

import { sleep } from "../lib/demo-instance.mjs";
import { DEMO_PIPELINE_ID, readTarget, withNodeFlags } from "../lib/targets.mjs";
import { installTargetPipeline } from "./_canvas.mjs";
import { openRun, startDemoRun, stopDemoRun, waitNode, waitPane } from "./_live.mjs";

/** The two parts, each a self-sufficient task for one child run. */
export const PARTS = [
  {
    name: "Product search",
    input: "Add a search box above the product list that filters the products by name as you type. Implement it yourself, in this run: do not create child runs.",
  },
  {
    name: "Price sort",
    input: "Add a select above the product list that sorts the products by price (low to high, high to low). Implement it yourself, in this run: do not create child runs.",
  },
];

/** The parent's task: orchestrate, never implement. The commands are spelled
 *  out so the agent starts both children at once, without a planning detour. */
export const ORCHESTRATION_TASK = {
  name: "Search and sort",
  input: [
    "Add two things to the shop's product list: a search box that filters the products by name as you type, and a select that sorts them by price.",
    "You orchestrate this one: do not write the code yourself. Start one child run of this same pipeline per part, right away, one after the other:",
    "",
    ...PARTS.map((part) => `pdo run create ${DEMO_PIPELINE_ID} --name "${part.name}" --input "${part.input}"`),
    "",
    "Then wait for both with pdo run wait --all, and complete.",
  ].join("\n"),
};

/** The demo pipeline with Orchestrator on for `implementer` — nothing else changes. */
export function orchestratorYaml(yaml = readTarget(DEMO_PIPELINE_ID).yaml) {
  return withNodeFlags(yaml, "implementer", { orchestrator: true });
}

const TERMINAL = new Set(["completed", "failed", "skipped", "halted", "archived"]);

/** The child runs of the run's nodes, with their status. */
async function childRuns(instance, runId) {
  const tree = await instance.api("GET", `/runs/${encodeURIComponent(runId)}/children`);
  return (tree.nodes ?? []).flatMap((node) => node.children ?? []);
}

/** Poll until `test(children)` holds. */
async function waitChildren(instance, runId, test, { timeout = 15 * 60_000 } = {}) {
  const deadline = Date.now() + timeout;
  for (;;) {
    const children = await childRuns(instance, runId);
    if (test(children)) return children;
    if (children.some((c) => c.status === "failed")) throw new Error(`a child run failed: ${children.map((c) => `${c.name} ${c.status}`).join(", ")}`);
    if (Date.now() > deadline) throw new Error(`children of ${runId}: ${JSON.stringify(children.map((c) => [c.name, c.status]))} after ${timeout} ms`);
    await sleep(250);
  }
}
const finished = (n) => (children) => children.filter((c) => TERMINAL.has(c.status)).length >= n;
const created = (n) => (children) => children.length >= n;

/** Open the orchestrating run, off camera, once Claude Code is up
 *  in its `implementer` (before it creates anything). The pane is read before
 *  the UI attaches to it: a narrow inspector would wrap its status line. */
async function openOrchestration(ctx, runId) {
  await waitPane(ctx.instance, runId, "implementer", /Opus 5\.5 · claude-opus-5-5/, { timeout: 90_000 });
  await openRun(ctx, ORCHESTRATION_TASK.name);
  // Off the rail: a hovered row shows its select ring and row actions.
  const canvas = await ctx.page.getByTestId("rf__wrapper").boundingBox();
  await ctx.moveTo({ x: canvas.x + canvas.width * 0.7, y: canvas.y + canvas.height * 0.35 }, { duration: 150 });
  await sleep(300);
}

/** Film the wait for the first children, fast-forwarded. */
async function fastUntilChildren(ctx, runId) {
  await ctx.fast(() => waitChildren(ctx.instance, runId, created(PARTS.length), { timeout: 5 * 60_000 }), { speed: 8 });
  // The rows land in the tree and the tab (the list polls).
  await ctx.page.locator('[data-depth="1"]').nth(PARTS.length - 1).waitFor({ timeout: 15_000 });
  await sleep(300);
}

/** The children at work: a short ×8 glimpse, then cut until the first one is done. */
async function fastThenCutUntilFirstDone(ctx, runId) {
  await ctx.fast(() => sleep(2400), { speed: 8 });
  await waitChildren(ctx.instance, runId, finished(1));
  await sleep(1500);
}

/** Off camera, once `implementer` is done: re-select it, so its ended session
 *  folds to the Terminal bar (#346, seeded when the panel mounts) instead of
 *  an empty « [exited] » block, and bring its Orchestration tab back. The
 *  cursor then rests on the canvas (a hovered row stays lit on the poster). */
async function showSettledImplementer(ctx, runId) {
  const { page, instance } = ctx;
  await waitNode(instance, runId, "implementer", { statuses: ["completed"], timeout: 3 * 60_000 });
  // Through `Start`, outside the loop region: a click on the reviewer that has
  // just started, inside it, may not select it, and then the panel never remounts.
  await ctx.click(page.getByTestId("rf__node-start"), { duration: 150, pause: 40 });
  await page.getByText("Run start").first().waitFor({ timeout: 10_000 });
  await sleep(400);
  await ctx.click(page.getByTestId("rf__node-implementer"), { duration: 150, pause: 40 });
  await page.getByTestId("terminal-minimized").waitFor({ timeout: 10_000 });
  const tab = page.getByTestId("detail-tab-orchestration");
  if ((await tab.getAttribute("data-active")) !== "true") await ctx.click(tab, { duration: 150, pause: 40 });
  await page.getByTestId("orchestration-child").nth(PARTS.length - 1).waitFor({ timeout: 15_000 });
  await sleep(600);
  const canvas = await page.getByTestId("rf__wrapper").boundingBox();
  await ctx.keep(() => ctx.hover({ x: canvas.x + canvas.width * 0.75, y: canvas.y + canvas.height * 0.3 }, { duration: 600, pause: 100 }));
}

export default {
  name: "orchestration",
  title: "Recursive orchestration — the demo pipeline relaunching itself, one child per part",
  needs: ["history"],
  live: ["claude"],
  async setup(instance) {
    // Same pipeline, same name: only the Orchestrator toggle of `implementer` differs.
    await installTargetPipeline(instance, DEMO_PIPELINE_ID, { yaml: orchestratorYaml() });
  },
  variants: [
    {
      id: "a",
      label: "Run tree on the left, implementer's Orchestration tab on the right: two children, counted to the end",
      viewport: { width: 1180, height: 680 },
      localStorage: { "pdo.layout.run": { left: 20, center: 38, right: 42 } },
      markers: ["child-created", "counters"],
      async play(ctx) {
        const { page, instance } = ctx;
        let runId;
        try {
          runId = await startDemoRun(instance, ORCHESTRATION_TASK);
          await openOrchestration(ctx, runId);
          await ctx.keep(async () => {
            await ctx.click(page.getByTestId("rf__node-implementer"), { duration: 750 });
            const tab = page.getByTestId("detail-tab-orchestration");
            await tab.waitFor({ timeout: 10_000 });
            await sleep(400);
            await ctx.click(tab, { duration: 700 });
            await sleep(500);
          });
          await fastUntilChildren(ctx, runId);
          await page.getByTestId("orchestration-child").nth(PARTS.length - 1).waitFor({ timeout: 15_000 });
          ctx.mark("child-created", { before: 200, after: 1800 });
          await sleep(1800);
          await fastThenCutUntilFirstDone(ctx, runId);
          ctx.mark("counters", { before: 400, after: 1400 });
          await sleep(1400);
          // Cut until the second child is done too: every counter ends « finished ».
          await waitChildren(instance, runId, finished(PARTS.length));
          await page.getByTestId("tab-child-pills-running").waitFor({ state: "detached", timeout: 15_000 });
          await showSettledImplementer(ctx, runId);
          await ctx.hold(2400);
        } finally {
          await stopDemoRun(instance, runId, { children: true, archive: true });
        }
      },
    },
    {
      id: "b",
      label: "Run tree and canvas: the children nest under their parent, the parent folds, its counters move",
      viewport: { width: 1000, height: 640 },
      localStorage: { "pdo.layout.run": { left: 28, center: 52, right: 20 } },
      // The run tree and the canvas; the inspector is cut.
      crop: { x: 0, y: 0, width: 800, height: 640 },
      markers: ["child-created", "counters"],
      async play(ctx) {
        const { page, instance } = ctx;
        let runId;
        try {
          runId = await startDemoRun(instance, ORCHESTRATION_TASK);
          await openOrchestration(ctx, runId);
          await ctx.keep(() => sleep(700));
          await fastUntilChildren(ctx, runId);
          ctx.mark("child-created", { before: 200, after: 1500 });
          await sleep(1500);
          await ctx.keep(async () => {
            // Fold the parent: its row counts the children now.
            const parent = page.locator('[data-depth="0"]').filter({ hasText: ORCHESTRATION_TASK.name }).first();
            await ctx.click(parent.getByTestId("run-tree-chevron"), { duration: 800 });
            await sleep(900);
          });
          await fastThenCutUntilFirstDone(ctx, runId);
          ctx.mark("counters", { before: 400, after: 1400 });
          await sleep(1400);
          await waitChildren(instance, runId, finished(PARTS.length));
          await page.getByTestId("run-child-pills-running").waitFor({ state: "detached", timeout: 15_000 });
          await sleep(600);
          // The cursor rests on the canvas, off the rail (a hovered row stays lit).
          const canvas = await page.getByTestId("rf__wrapper").boundingBox();
          await ctx.keep(() => ctx.hover({ x: canvas.x + canvas.width * 0.75, y: canvas.y + canvas.height * 0.3 }, { duration: 600, pause: 100 }));
          await ctx.hold(2400);
        } finally {
          await stopDemoRun(instance, runId, { children: true, archive: true });
        }
      },
    },
  ],
};
