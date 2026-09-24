// Scene « interactive-orchestrator » (row « Interactive & orchestrator nodes »):
// the two node kinds in one story, on the hero's pipeline. `implementer` is
// interactive AND orchestrator (`interactiveOrchestratorYaml`: the target
// `implement-review`, both flags and their skills on for that node, every
// other byte as drawn). Its REAL agent asks a question and declares its wait
// (`pdo wait-user`): the node turns « awaiting you », its banner carries the
// question. The cursor types the answer in its terminal, the Enter lifts the
// wait, and the agent starts two child runs of `implement-review`
// (`pdo run create`), which nest under it in the run tree while the counters
// follow them.
//
//   awaiting-you  — the banner « The agent is waiting for your answer », the question in the terminal
//   answer-typed  — the answer typed in the terminal, Enter: the banner lifts
//   child-created — the two children, nested under their parent in the run tree
//   counters      — (b) the first child finished: the counters move
//
//   a — the whole window: the run tree, the canvas, and `implementer`'s
//       inspector (the banner above its live terminal). Ends on the two
//       children running.
//   b — the same story, then a cut until the first child is done: the
//       counters (the parent row, the node, the Orchestration tab) move.
//
// The children run the PLAIN `implement-review`: once the parent's
// `implementer` has started (its run froze its own snapshot, flags included),
// the library gets the target back, so the children neither ask nor
// orchestrate. Each variant plays its own run, and stops and archives it with
// its children (every agent: the parent, the children, their manager) once
// filmed.

import { sleep } from "../lib/demo-instance.mjs";
import { DEMO_PIPELINE_ID, readTarget, withNodeFlags } from "../lib/targets.mjs";
import { installTargetPipeline, restoreDemoPipeline } from "./_canvas.mjs";
import { nodeState, openRun, paneText, startDemoRun, stopDemoRun, waitNode, waitPane } from "./_live.mjs";

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

/** The question `implementer` asks (the banner's message, under 100 characters),
 *  and the answer the cursor types in its terminal. */
export const QUESTION = "Search and price sort: one child run each, in parallel?";
export const ANSWER = "Yes, both in parallel.";

/** The one wait the parent runs once its children are started: a bare command
 *  on camera, not the `while ! pdo run wait …` loop of the orchestrate skill.
 *  Its timeout outlasts the filmed variant. */
export const WAIT_COMMAND = "pdo run wait --all --timeout 590";

/** The parent's task: ask first, then orchestrate, never implement. The
 *  commands are spelled out so the agent asks once and starts both children at
 *  once, without a planning detour. */
export const INTERACTIVE_TASK = {
  name: "Search and sort",
  input: [
    "Add two things to the shop's product list: a search box that filters the products by name as you type, and a select that sorts them by price.",
    "You orchestrate this one: do not write the code yourself, and do not explore the repository.",
    "",
    "First, ask me before you start anything. Declare your wait, then ask me this exact question in one line, and end your turn:",
    "",
    `pdo wait-user --message "${QUESTION}"`,
    "",
    "Once I answer yes, start one child run of this same pipeline per part, right away, one after the other:",
    "",
    ...PARTS.map((part) => `pdo run create ${DEMO_PIPELINE_ID} --name "${part.name}" --input "${part.input}"`),
    "",
    "Then wait for both with this one command, no loop around it:",
    "",
    WAIT_COMMAND,
    "",
    "Run every command exactly as written, from your current directory: no cd, no absolute path.",
  ].join("\n"),
};

/** The node's two flags, and the skill each toggle adds (as the editor's toggles do). */
export const FLAGS = {
  interactive: true,
  orchestrator: true,
  skills: "[{ id: pdo-interactive, name: pdo-interactive }, { id: pdo-orchestrate, name: pdo-orchestrate }]",
};

/** The demo pipeline with `implementer` interactive and orchestrator — nothing else changes. */
export function interactiveOrchestratorYaml(yaml = readTarget(DEMO_PIPELINE_ID).yaml) {
  return withNodeFlags(yaml, "implementer", FLAGS);
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

/** Poll until the daemon serves `implementer` with `interactive` set to `on`
 *  (its node and edge counts never change, so the count check cannot tell). */
async function waitInteractiveServed(instance, on) {
  const deadline = Date.now() + 15_000;
  for (;;) {
    const { pipeline } = await instance.api("GET", `/pipelines/${DEMO_PIPELINE_ID}`);
    const node = pipeline.nodes?.find((n) => n.id === "implementer");
    if (Boolean(node?.interactive) === on && Boolean(node?.orchestrator) === on) return;
    if (Date.now() > deadline) throw new Error(`the daemon never served implementer with interactive/orchestrator ${on}`);
    await sleep(200);
  }
}

/** Off camera: the flagged pipeline in, the run started, and — once Claude Code
 *  is up in `implementer` — the plain target back in the library, for the
 *  children. Then the run open, `implementer`'s terminal attached. */
async function startInteractiveRun(ctx) {
  const { instance, page } = ctx;
  await installTargetPipeline(instance, DEMO_PIPELINE_ID, { yaml: interactiveOrchestratorYaml() });
  await waitInteractiveServed(instance, true);
  const runId = await startDemoRun(instance, INTERACTIVE_TASK);
  // The pane is read before the UI attaches to it: a narrow inspector would wrap its status line.
  await waitPane(instance, runId, "implementer", /Opus 5\.5 · claude-opus-5-5/, { timeout: 90_000 });
  const node = await nodeState(instance, runId, "implementer");
  if (!node) throw new Error("implementer never started");
  await restoreDemoPipeline(instance);
  await waitInteractiveServed(instance, false);
  // Opening the run selects its live node: `implementer`, its terminal attached.
  await openRun(ctx, INTERACTIVE_TASK.name);
  await page.getByText("attached · live").waitFor({ timeout: 30_000 });
  await restCursor(ctx);
  return runId;
}

/** The cursor rests on the canvas, off the rail (a hovered row shows its ring and actions). */
async function restCursor(ctx, { filmed = false } = {}) {
  const canvas = await ctx.page.getByTestId("rf__wrapper").boundingBox();
  const point = { x: canvas.x + canvas.width * 0.72, y: canvas.y + canvas.height * 0.3 };
  if (filmed) await ctx.keep(() => ctx.hover(point, { duration: 600, pause: 100 }));
  else await ctx.moveTo(point, { duration: 150 });
}

/** Film the agent reading its task (×8, capped), then cut until it has
 *  declared its wait and printed its question: the node awaiting, the banner
 *  up, the terminal still for a moment (its turn ended). */
async function untilQuestion(ctx, runId) {
  const { instance, page } = ctx;
  const awaiting = waitNode(instance, runId, "implementer", { statuses: ["awaiting_user"], timeout: 5 * 60_000 });
  await ctx.fast(() => Promise.race([awaiting, sleep(16_000)]), { speed: 8 });
  const node = await awaiting;
  if (node.status !== "awaiting_user") throw new Error(`implementer is ${node.status}, not awaiting its user`);
  await page.getByTestId("awaiting-banner-message").filter({ hasText: QUESTION.slice(0, 30) }).waitFor({ timeout: 15_000 });
  // The question in the pane, and the pane settled: Claude Code gave its turn back.
  let last = "";
  let stableSince = Date.now();
  const deadline = Date.now() + 90_000;
  while (Date.now() < deadline) {
    const text = paneText(instance, runId, "implementer");
    if (text !== last) {
      last = text;
      stableSince = Date.now();
    } else if (Date.now() - stableSince > 2500 && /parallel\?/.test(text)) break;
    await sleep(250);
  }
  await sleep(300);
}

/** How long the typed answer stays on screen before the Enter. */
export const ANSWER_HOLD_MS = 1000;

/** What the browser's terminal paints (xterm's DOM renderer), spaces normalized. */
export async function screenText(page) {
  const text = await page.locator('[data-testid="xterm-container"] .xterm-rows').first().innerText();
  return text.replace(/\u00a0/g, " ");
}

/** Wait until the browser's terminal paints `text`: tmux echoes a key a few
 *  hundred ms before the WebSocket bridge brings it to the page. */
async function untilOnScreen(page, text, timeout = 5_000) {
  const deadline = Date.now() + timeout;
  while (!(await screenText(page)).includes(text)) {
    if (Date.now() > deadline) throw new Error(`« ${text} » never showed in the browser's terminal`);
    await sleep(40);
  }
}

/** Filmed: a click in the terminal, the answer typed a word at a time (a key
 *  per character round-trips through the PTY bridge and drags over seconds).
 *  Each word waits for the pane to echo it and the page to paint it, so the
 *  words land one by one instead of all at once after a lag; the whole answer
 *  stays up `ANSWER_HOLD_MS` on screen before the Enter. */
async function typeAnswer(ctx, runId) {
  const { page, instance } = ctx;
  await ctx.click(page.getByTestId("xterm-container"), { duration: 700 });
  await sleep(150);
  let typed = "";
  for (const word of ANSWER.match(/\S+\s*/g)) {
    await page.keyboard.insertText(word);
    typed += word;
    const echoed = typed.trimEnd();
    const deadline = Date.now() + 5_000;
    while (!paneText(instance, runId, "implementer").includes(echoed)) {
      if (Date.now() > deadline) throw new Error(`« ${echoed} » never reached the implementer's terminal`);
      await sleep(50);
    }
    await untilOnScreen(page, echoed);
    await sleep(180);
  }
  await sleep(ANSWER_HOLD_MS);
  await page.keyboard.press("Enter");
}

/** The Enter lifts the wait: the node runs again and its banner goes (×8, the UI polls). */
async function fastUntilLifted(ctx, runId) {
  const { page, instance } = ctx;
  await ctx.fast(async () => {
    await waitNode(instance, runId, "implementer", { statuses: ["running"], timeout: 15_000 });
    await page.getByTestId("awaiting-banner").waitFor({ state: "detached", timeout: 15_000 });
  }, { speed: 8 });
}

/** Film the agent starting its children, fast-forwarded, until both rows sit in the tree. */
async function fastUntilChildren(ctx, runId) {
  await ctx.fast(() => waitChildren(ctx.instance, runId, created(PARTS.length), { timeout: 5 * 60_000 }), { speed: 8 });
  await ctx.page.locator('[data-depth="1"]').nth(PARTS.length - 1).waitFor({ timeout: 15_000 });
  await sleep(400);
}

/** The demo root never shows on camera: a command the agent ran with the
 *  instance's absolute path (a `cd /tmp/pdo-readme-media-…`) fails the variant. */
export function assertNoDemoPath(text) {
  // Joined: the pane wraps a long command, maybe in the middle of the path.
  if (/\/tmp\/pdo-readme-media/.test(text.replace(/\n/g, ""))) throw new Error("the implementer's terminal shows the demo root (/tmp/pdo-readme-media-…)");
}

/** The shared story: the question, the answer, the children. */
async function playStory(ctx, runId) {
  await untilQuestion(ctx, runId);
  ctx.mark("awaiting-you", { before: 300, after: 1600 });
  await sleep(1600);
  await ctx.keep(() => typeAnswer(ctx, runId));
  ctx.mark("answer-typed", { before: 0, after: 500 });
  await sleep(500);
  await fastUntilLifted(ctx, runId);
  await fastUntilChildren(ctx, runId);
  assertNoDemoPath(paneText(ctx.instance, runId, "implementer"));
  ctx.mark("child-created", { before: 200, after: 1600 });
  await sleep(1600);
}

export default {
  name: "interactive-orchestrator",
  title: "Interactive & orchestrator nodes — a question, the answer typed in the terminal, two child runs",
  needs: ["history"],
  live: ["claude"],
  variants: [
    {
      id: "a",
      label: "Whole window: the question on the banner, the answer typed in the terminal, two children in the run tree",
      viewport: { width: 1180, height: 680 },
      localStorage: { "pdo.layout.run": { left: 21, center: 31, right: 48 } },
      markers: ["awaiting-you", "answer-typed", "child-created"],
      async play(ctx) {
        let runId;
        try {
          runId = await startInteractiveRun(ctx);
          await playStory(ctx, runId);
          await restCursor(ctx, { filmed: true });
          await ctx.hold(1900);
          assertNoDemoPath(paneText(ctx.instance, runId, "implementer"));
        } finally {
          await stopDemoRun(ctx.instance, runId, { children: true, archive: true });
        }
      },
    },
    {
      id: "b",
      label: "Run tree and inspector: the question, the answer, two children, then the first one finished on the counters",
      viewport: { width: 1100, height: 660 },
      // The canvas keeps ~360 px: narrower, the loop edge's labels spill into the inspector.
      localStorage: { "pdo.layout.run": { left: 22, center: 33, right: 45 } },
      markers: ["awaiting-you", "answer-typed", "child-created", "counters"],
      async play(ctx) {
        const { page, instance } = ctx;
        let runId;
        try {
          runId = await startInteractiveRun(ctx);
          await playStory(ctx, runId);
          // A short ×8 glimpse of the children at work, then cut until the first is done.
          await ctx.fast(() => sleep(2400), { speed: 8 });
          await waitChildren(instance, runId, finished(1));
          // The rail polls: its parent row counts the finished child.
          await page.getByTestId("run-child-pills-finished").waitFor({ timeout: 30_000 });
          await sleep(1200);
          ctx.mark("counters", { before: 300, after: 600 });
          await restCursor(ctx, { filmed: true });
          await ctx.hold(1600);
          assertNoDemoPath(paneText(instance, runId, "implementer"));
        } finally {
          await stopDemoRun(instance, runId, { children: true, archive: true });
        }
      },
    },
  ],
};
