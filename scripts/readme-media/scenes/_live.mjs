// Helpers for the LIVE scenes (a real run of the demo pipeline `implement-review`,
// real `claude` / `claude-opus-5-5` agents). Not a scene: the `_` prefix keeps
// it out of scene discovery. Reused by the hero (#858) and the typed-outputs /
// diff-review / orchestration scenes (#859).
//
// Every live variant starts its run through `startDemoRun` and ends it through
// `stopDemoRun` in a `finally`: the variant's agents are stopped as soon as it
// is filmed, even when it fails. The scene's teardown (tmux kill-server, then
// waiting for every agent to exit) stays the backstop for the whole scene.
//
// A scene that films a FINISHED run (outputs, review) plays it once in its
// `setup`, off camera, with `completeDemoRun`: both variants film the same run.

import { execFileSync } from "node:child_process";
import { sleep } from "../lib/demo-instance.mjs";
import { DEMO_PIPELINE } from "../lib/demo-pipeline.mjs";

/** The feature the demo `implementer` adds to the fixture shop (small on purpose). */
export const DEMO_TASK = {
  name: "Add a product search",
  input: "Add a search box above the product list that filters the products by name as you type, with a short message when nothing matches.",
};

// A node that will not move on its own any more (`interrupted` parks the run).
const TERMINAL = new Set(["completed", "skipped", "failed", "stopped", "stale", "interrupted", "awaiting_user"]);

/** Start a run of the demo pipeline on the fixture repo. Returns its id. */
export async function startDemoRun(instance, task = DEMO_TASK) {
  const { run_id: runId } = await instance.api("POST", "/runs", {
    pipeline: DEMO_PIPELINE.id,
    name: task.name,
    input: task.input,
    target_repo: instance.repo,
  });
  return runId;
}

/** Play a whole run of the demo pipeline, off camera, and return its id once
 *  `reviewer` completed and the run with it. Its node sessions have exited by
 *  then; whatever is left is stopped. Throws when the run did not complete. */
export async function completeDemoRun(instance, task = DEMO_TASK, { timeout = 20 * 60_000 } = {}) {
  const runId = await startDemoRun(instance, task);
  try {
    const reviewer = await waitNode(instance, runId, "reviewer", { timeout });
    if (reviewer.status !== "completed") throw new Error(`the demo run ${runId} ended with reviewer ${reviewer.status}`);
    await waitRun(instance, runId, ["completed"], { timeout: 60_000 });
  } catch (error) {
    await stopDemoRun(instance, runId);
    throw error;
  }
  await stopDemoRun(instance, runId);
  return runId;
}

export async function nodeState(instance, runId, nodeId) {
  const run = await instance.api("GET", `/runs/${encodeURIComponent(runId)}`);
  return run.nodes?.[nodeId] ?? null;
}

/** Poll until `nodeId` is in one of `statuses` (default: any terminal one). */
export async function waitNode(instance, runId, nodeId, { statuses, timeout = 15 * 60_000, every = 250 } = {}) {
  const wanted = statuses ? new Set(statuses) : TERMINAL;
  const deadline = Date.now() + timeout;
  for (;;) {
    const node = await nodeState(instance, runId, nodeId);
    if (node && wanted.has(node.status)) return node;
    if (Date.now() > deadline) throw new Error(`${nodeId} of run ${runId} still ${node?.status ?? "absent"} after ${timeout} ms`);
    await sleep(every);
  }
}

/** Poll until the run itself is in one of `statuses`. */
export async function waitRun(instance, runId, statuses, { timeout = 60_000, every = 250 } = {}) {
  const deadline = Date.now() + timeout;
  for (;;) {
    const run = await instance.api("GET", `/runs/${encodeURIComponent(runId)}`);
    if (statuses.includes(run.status)) return run;
    if (Date.now() > deadline) throw new Error(`run ${runId} still ${run.status} after ${timeout} ms`);
    await sleep(every);
  }
}

/** The text of a node's live tmux pane on the demo socket ("" when there is none). */
export function paneText(instance, runId, nodeId, iter = 1) {
  try {
    return execFileSync("tmux", ["-L", instance.tmuxSocket, "capture-pane", "-p", "-t", `pdo-${runId}-${nodeId}-iter-${iter}`], {
      encoding: "utf8",
      stdio: ["ignore", "pipe", "ignore"],
    });
  } catch {
    return "";
  }
}

/** Wait until the pane of a live node shows `pattern` (Claude Code is up and working). */
export async function waitPane(instance, runId, nodeId, pattern, { timeout = 60_000 } = {}) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    if (pattern.test(paneText(instance, runId, nodeId))) return;
    await sleep(200);
  }
  throw new Error(`the ${nodeId} pane never showed ${pattern}`);
}

/** The child runs a run's nodes created, then theirs (depth first). */
export async function descendantRuns(instance, runId) {
  const out = [];
  let tree = null;
  try {
    tree = await instance.api("GET", `/runs/${encodeURIComponent(runId)}/children`);
  } catch {
    return out;
  }
  for (const node of tree?.nodes ?? []) {
    for (const child of node.children ?? []) {
      out.push(child.run_id);
      out.push(...(await descendantRuns(instance, child.run_id)));
    }
  }
  return out;
}

/** Stop every agent of the run now: the running nodes through the API, then
 *  any session of the run still on the demo tmux socket, its manager's
 *  included (`pdo-mgr-<run>`, started on demand by an « Envoi au manager »).
 *  `children` does the same for every run the run's nodes created, first.
 *  `archive` also cleans the runs up, so a stopped take leaves the runs rail of
 *  the next variant. Never throws. */
export async function stopDemoRun(instance, runId, { archive = false, children = false } = {}) {
  if (!runId) return;
  if (children) {
    for (const child of await descendantRuns(instance, runId)) await stopDemoRun(instance, child, { archive });
  }
  try {
    const run = await instance.api("GET", `/runs/${encodeURIComponent(runId)}`);
    for (const [nodeId, node] of Object.entries(run.nodes ?? {})) {
      if (node.status === "running" || node.status === "awaiting_user") {
        await instance.api("POST", `/runs/${encodeURIComponent(runId)}/nodes/${encodeURIComponent(nodeId)}/stop`).catch(() => {});
      }
    }
  } catch {
    // the daemon is gone: the scene teardown kills the tmux server anyway
  }
  let sessions = [];
  try {
    sessions = execFileSync("tmux", ["-L", instance.tmuxSocket, "list-sessions", "-F", "#S"], { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] })
      .split("\n")
      .filter((s) => s.startsWith(`pdo-${runId}-`) || s === `pdo-mgr-${runId}`);
  } catch {
    // no tmux server: nothing left running
  }
  for (const session of sessions) {
    try {
      execFileSync("tmux", ["-L", instance.tmuxSocket, "kill-session", "-t", session], { stdio: "ignore" });
    } catch {
      // already gone
    }
  }
  if (archive) {
    await instance.api("POST", `/runs/${encodeURIComponent(runId)}/commands`, { kind: "cleanup_run" }).catch((error) => {
      console.warn(`   could not archive run ${runId}: ${error.message}`);
    });
  }
}

/** Open the runs rail (history loaded, never « No runs yet ») and select the run
 *  named `name`, off camera. Resolves once its canvas shows `implementer`. */
export async function openRun(ctx, name = DEMO_TASK.name) {
  const { page } = ctx;
  await ctx.goto("/");
  const row = page.getByTestId("run-display-label").filter({ hasText: name }).first();
  await row.waitFor({ timeout: 30_000 });
  // Through ctx, even off camera: the cursor overlay follows the real mouse,
  // and the recorder must know where it is for the next filmed gesture.
  await ctx.click(row, { duration: 150, pause: 40 });
  await page.getByTestId("rf__node-implementer").waitFor({ timeout: 30_000 });
  await sleep(600);
}

/** Wheel-scroll the panel under `over` (a locator or a point) until `target`'s
 *  top sits at `top` px of the page, in one eased, filmed gesture. */
export async function scrollUntil(ctx, target, { over, top, duration = 800 } = {}) {
  await ctx.moveTo(over ?? target, { duration: 500 });
  const box = await target.boundingBox();
  if (!box) throw new Error(`cannot scroll to ${target}: not rendered`);
  const dy = Math.round(box.y - top);
  if (Math.abs(dy) > 4) await ctx.scroll(dy, { duration });
  await sleep(350);
}
