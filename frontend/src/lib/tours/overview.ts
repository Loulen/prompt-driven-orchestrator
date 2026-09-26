/**
 * The *Overview* tour (#911, spec #910) — the **tour préface**: a walk around the
 * screen on a real, finished Run, before *First run* teaches a single gesture in
 * depth. It needs **no harness**: the Run it visits is a pipeline of `script`
 * nodes (ADR-0017), which finish in seconds and cost nothing.
 *
 * Seventeen steps, seven of them gestures that advance only on the observed state
 * (open the Run, click implementer, the reviewer → End edge, Start, archive,
 * Triggers, Pipelines); the other ten are read and acknowledged with Next.
 *
 * Its intro card prepares four things through verbs that know nothing about tours
 * (ADR-0071 §3–4): the training repository, the `tutorial-overview` pipeline, an
 * example Trigger that is harmless by construction, and a completed Run. Its
 * teardown removes the Trigger at any exit; the pipeline and the Run stay the
 * user's, as after *First run*.
 *
 * Copy rules inherited from the other tours: title is an imperative, body is two
 * sentences at most, and the other tours are pointed to in one sentence, never a
 * button.
 */

import {
  ApiError,
  createPipeline,
  createRun,
  createTrigger,
  deleteTrigger,
  fetchPipelines,
  fetchRun,
  fetchRuns,
  fetchTriggers,
  savePipeline,
  updateTrigger,
} from "../../api";
import type { NodeDef } from "../../types";
import type { TourAppState, TourDef, TourObservation, TourRunSummary, TourStep } from "../tour";
import { prepareRepo, TUTORIAL_REPO_PATH } from "./firstRun";

/** The pipeline the tour's Run executes. A normal Library pipeline, kept after the tour. */
export const TUTORIAL_OVERVIEW_PIPELINE_ID = "tutorial-overview";

/** The name the tour gives its Run, so the list says what the row is. */
export const OVERVIEW_RUN_NAME = "tour-overview";

/** The Run's prompt — fixed, and what the Start step shows the reader. */
export const OVERVIEW_PROMPT =
  "Show me around PDO: implement a tiny change, then review it and hand back a verdict.";

/** The Trigger the tour installs to be shown, and removes on the way out. */
export const EXAMPLE_TRIGGER_NAME = "tutorial-example-trigger";

/** Every 15 minutes — a schedule anyone can read at a glance. */
const EXAMPLE_TRIGGER_CRON = "*/15 * * * *";

/**
 * The guard that makes the Trigger harmless **by construction** (ADR-0071 §4):
 * `false` exits non-zero, so every fire is skipped and no Run is ever created —
 * even when a tab closed mid-tour left the Trigger behind.
 */
const EXAMPLE_TRIGGER_GUARD = "false";

const EXAMPLE_TRIGGER_INPUT =
  "An example input. This Trigger's guard refuses every fire, so it never starts a Run.";

// ---- the pipeline -----------------------------------------------------------

/**
 * `implement-review`'s layout (the README media fixture: positions, anchors, edge
 * routes), with two `script` nodes in place of the agents. Scripts run in the
 * Run worktree and only ever write their outputs, so the worktree stays clean.
 * No `image_list` port: a script has nothing to take a screenshot of.
 *
 * Edge order matters to the tour: the canvas keys an edge by its index, and the
 * edge step points at reviewer → End (`verdict eq pass`), the one the Run takes.
 */
export const OVERVIEW_PIPELINE_YAML = `name: ${TUTORIAL_OVERVIEW_PIPELINE_ID}
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - { name: user_prompt, side: right }
    view: { x: 199, y: 47 }
  - id: end
    name: End
    type: end
    inputs:
      - { name: result, side: left }
    view: { x: 204, y: 316 }
  - id: implementer
    name: implementer
    type: script
    isolated_worktree: false
    inputs:
      - { name: task, side: left }
    outputs:
      - name: code
        side: right
        instructions: "What was changed, in a few lines."
    view: { x: 196, y: 135 }
  - id: reviewer
    name: reviewer
    type: script
    isolated_worktree: false
    inputs:
      - { name: code, side: left }
    outputs:
      - name: review
        side: right
        frontmatter:
          verdict:
            allowed: [pass, fail]
            type: enum
        instructions: "Your verdict, then a small Mermaid diagram of the flow you checked."
    view: { x: 197, y: 208 }
edges:
  - source: { node: start, port: user_prompt }
    target: { node: implementer, port: task }
    mode: manual
    waypoints:
      - { x: 280, y: 95 }
    target_side: top
    source_anchor: { side: bottom, offset: 81 }
    target_anchor: { side: top, offset: 84 }
  - source: { node: implementer, port: code }
    target: { node: reviewer, port: code }
    mode: manual
    waypoints:
      - { x: 280, y: 168 }
    target_side: top
    source_anchor: { side: bottom, offset: 84 }
    target_anchor: { side: top, offset: 83 }
  - source: { node: reviewer, port: review }
    target: { node: implementer, port: task }
    when:
      verdict: { eq: fail }
    mode: manual
    waypoints:
      - { x: 397, y: 154 }
      - { x: 396, y: 154 }
    target_side: right
    source_anchor: { side: right, offset: 26 }
    target_anchor: { side: right, offset: 19 }
    output_label_pos:
      review: { x: 381, y: 225 }
    condition_label_pos: { x: 395, y: 192 }
  - source: { node: reviewer, port: review }
    target: { node: end, port: result }
    when:
      verdict: { eq: pass }
    mode: manual
    waypoints:
      - { x: 280, y: 276 }
    target_side: top
    source_anchor: { side: bottom, offset: 83 }
    target_anchor: { side: top, offset: 76 }
    output_label_pos:
      review: { x: 253, y: 256 }
    condition_label_pos: { x: 284, y: 284 }
loops:
  - id: loop-tutorial-overview
    kind: bounded
    members: [implementer, reviewer]
    max_iter: 3
`;

/**
 * The implementer's bash body (ADR-0017: stored in the node's prompt slot, run as
 * `timeout 60s bash <body>`). It prints what the frozen terminal pane will show,
 * and writes its one output.
 *
 * It completes **itself**, from inside its session, and waits there to be reaped
 * (#911). The daemon snapshots a node's pane when it reaps the session, after a
 * completion; the wrapper's own `pdo complete` runs only once the script has
 * exited, and the session goes with it — so a script that simply ended would
 * leave the terminal step a « Session no longer available ». Should the reap be
 * slow, the wait gives up after 20 s, the wrapper completes again, and the
 * daemon answers it as a legal duplicate.
 */
export const IMPLEMENTER_SCRIPT = `#!/usr/bin/env bash
# tutorial-overview · implementer — a script node, so the tour needs no agent.
set -euo pipefail

echo "implementer: reading the task from Start"
echo "implementer: an agent would edit the repository here; this script only writes its output"
sleep 1

cat > "$PDO_OUTPUT_CODE" <<'OUT'
# What the implementer did

Nothing to the repository: this node is a script, so the tour costs nothing.
An agent node would have made the change here and described it in this file.
OUT

echo "implementer: wrote the code output — done"
# Hand the output over from inside the session, then wait to be reaped. The
# daemon freezes a node's pane when it reaps the session; the wrapper's own
# \`pdo complete\` only runs once this script has exited — and its session with
# it — so the tour's terminal step would have nothing to show.
pdo complete
# Then leave as soon as the pane is gone — polled, not slept: the reap hangs up
# the session's shell only, and a plain \`sleep\` would outlive it (20 s at most).
for _ in $(seq 1 100); do
  tmux display-message -p -t "\${TMUX_PANE:-}" "" >/dev/null 2>&1 || exit 0
  sleep 0.2
done
`;

/** The reviewer's body: `verdict: pass` in the frontmatter, and a mermaid diagram. */
export const REVIEWER_SCRIPT = `#!/usr/bin/env bash
# tutorial-overview · reviewer — a script node, so the tour needs no agent.
set -euo pipefail

echo "reviewer: reading the implementer's code output"
echo "reviewer: verdict pass — the edge to End fires"
sleep 1

cat > "$PDO_OUTPUT_REVIEW" <<'OUT'
---
verdict: pass
---
# Review

The change holds up. The edge reviewer → End reads this verdict and ends the Run.

\`\`\`mermaid
flowchart LR
  Start --> implementer --> reviewer
  reviewer -- verdict eq pass --> End
  reviewer -- verdict eq fail --> implementer
\`\`\`
OUT

echo "reviewer: wrote the review output — done"
# Hand the output over from inside the session, then wait to be reaped. The
# daemon freezes a node's pane when it reaps the session; the wrapper's own
# \`pdo complete\` only runs once this script has exited — and its session with
# it — so the tour's terminal step would have nothing to show.
pdo complete
# Then leave as soon as the pane is gone — polled, not slept: the reap hangs up
# the session's shell only, and a plain \`sleep\` would outlive it (20 s at most).
for _ in $(seq 1 100); do
  tmux display-message -p -t "\${TMUX_PANE:-}" "" >/dev/null 2>&1 || exit 0
  sleep 0.2
done
`;

// ---- preparation ------------------------------------------------------------

/**
 * What this pass of the tour prepared: the Run it visits and the Trigger it shows.
 * Set by the preparations, read by the steps — the Run is not always the newest
 * one in the list (a replay reuses an older one), so « the tour's Run » has to be
 * remembered rather than guessed.
 */
const prepared: { runId: string | null; triggerId: string | null } = { runId: null, triggerId: null };

/** The Run this pass of the tour is visiting, once its preparation has answered. */
export function overviewRunId(): string | null {
  return prepared.runId;
}

/** The example Trigger this pass of the tour installed or picked up. */
export function exampleTriggerId(): string | null {
  return prepared.triggerId;
}

/**
 * Share one in-flight call between the preparations that need it. They run in
 * parallel (the engine starts them together), but the Trigger and the Run can only
 * be made once the repository and the pipeline exist: both await the same
 * promise instead of racing a second `POST /repos/create`.
 */
function shared(fn: () => Promise<void>): () => Promise<void> {
  let flight: Promise<void> | null = null;
  return () => {
    flight ??= fn().finally(() => {
      flight = null;
    });
    return flight;
  };
}

const repoReady = shared(prepareRepo);

/**
 * Put `tutorial-overview` in the Library, or leave the one already there alone —
 * a user who edited it keeps their edit, the rule *First run* follows too.
 */
const pipelineReady = shared(async () => {
  const existing = await fetchPipelines();
  if (existing.some((p) => p.id === TUTORIAL_OVERVIEW_PIPELINE_ID)) return;
  try {
    await createPipeline(TUTORIAL_OVERVIEW_PIPELINE_ID);
  } catch (e) {
    // Lost the race with another tab: the pipeline exists, which is all we promised.
    if (!(e instanceof ApiError) || e.status !== 409) throw e;
    return;
  }
  await savePipeline(TUTORIAL_OVERVIEW_PIPELINE_ID, OVERVIEW_PIPELINE_YAML, {
    implementer: IMPLEMENTER_SCRIPT,
    reviewer: REVIEWER_SCRIPT,
  });
});

async function prepareRepoStep(): Promise<void> {
  await repoReady();
}

async function preparePipelineStep(): Promise<void> {
  await pipelineReady();
}

/**
 * The example Trigger, active, picked up by its name when a previous pass left
 * it behind (a tab closed mid-tour skips the teardown). A picked-up Trigger is
 * put back in the shape the tour shows — active, guard `false` — because it is the
 * tour's object, not the user's.
 */
export async function prepareExampleTrigger(): Promise<void> {
  await Promise.all([repoReady(), pipelineReady()]);
  const existing = (await fetchTriggers()).find((t) => t.name === EXAMPLE_TRIGGER_NAME);
  if (existing) {
    if (!existing.enabled || (existing.guard_command ?? "").trim() !== EXAMPLE_TRIGGER_GUARD) {
      await updateTrigger(existing.id, { enabled: true, guard_command: EXAMPLE_TRIGGER_GUARD });
    }
    prepared.triggerId = existing.id;
    return;
  }
  const created = await createTrigger({
    name: EXAMPLE_TRIGGER_NAME,
    pipeline_id: TUTORIAL_OVERVIEW_PIPELINE_ID,
    cron: EXAMPLE_TRIGGER_CRON,
    input_template: EXAMPLE_TRIGGER_INPUT,
    target_repo: TUTORIAL_REPO_PATH,
    guard_command: EXAMPLE_TRIGGER_GUARD,
    overlap_policy: "skip",
    auto_name: false,
  });
  prepared.triggerId = created.id;
}

/** How often the Run is re-read while the preparation waits for it. */
export const RUN_POLL_MS = 1_000;
/**
 * How long the preparation waits for the Run. Two scripts under the 60 s wrapper
 * timeout each: a Run still going after three minutes is stuck, and a card that
 * waited forever would be the infinite wait story 7 refuses.
 */
export const RUN_WAIT_MS = 180_000;

function abortError(): Error {
  return new DOMException("The tour was closed.", "AbortError");
}

function sleep(ms: number, signal?: AbortSignal): Promise<void> {
  return new Promise((resolve, reject) => {
    if (signal?.aborted) return reject(abortError());
    const timer = setTimeout(() => {
      signal?.removeEventListener("abort", onAbort);
      resolve();
    }, ms);
    const onAbort = () => {
      clearTimeout(timer);
      reject(abortError());
    };
    signal?.addEventListener("abort", onAbort, { once: true });
  });
}

/**
 * Read the Run until it is `completed`. Any other terminal or waiting state is a
 * refusal quoting the daemon's reason (spec #910): a tour that visited a failed
 * Run would be explaining outputs that were never written.
 */
async function waitForCompleted(runId: string, signal?: AbortSignal): Promise<void> {
  const deadline = Date.now() + RUN_WAIT_MS;
  for (;;) {
    if (signal?.aborted) throw abortError();
    const run = await fetchRun(runId);
    if (run.status === "completed") return;
    if (run.status !== "running") {
      const reason = run.failure_reason ?? run.awaiting_reason ?? null;
      throw new Error(
        `Run ${run.name?.trim() || runId} ended \`${run.status}\` instead of \`completed\`${reason ? `: ${reason}` : "."}`,
      );
    }
    if (Date.now() >= deadline) {
      throw new Error(
        `Run ${run.name?.trim() || runId} was still running after ${RUN_WAIT_MS / 60_000} minutes.`,
      );
    }
    await sleep(RUN_POLL_MS, signal);
  }
}

/**
 * The Run the tour visits (spec #910, design Q8): the newest `tutorial-overview`
 * Run that is `completed` and not archived — so replaying the tour does not stack
 * identical Runs — else a fresh one, waited for until `completed`.
 *
 * One still `running` (a previous pass closed before it finished) is waited for
 * rather than doubled.
 */
export async function prepareCompletedRun(signal?: AbortSignal): Promise<void> {
  prepared.runId = null;
  const runs = await fetchRuns();
  const ours = runs.filter((r) => r.pipeline_name === TUTORIAL_OVERVIEW_PIPELINE_ID && !r.triggered_by);
  const reusable = ours.find((r) => r.status === "completed") ?? ours.find((r) => r.status === "running");
  let runId = reusable?.run_id ?? null;
  if (!runId) {
    await Promise.all([repoReady(), pipelineReady()]);
    if (signal?.aborted) throw abortError();
    const created = await createRun({
      pipeline: TUTORIAL_OVERVIEW_PIPELINE_ID,
      pipeline_id: TUTORIAL_OVERVIEW_PIPELINE_ID,
      input: OVERVIEW_PROMPT,
      variables: {},
      target_repo: TUTORIAL_REPO_PATH,
      name: OVERVIEW_RUN_NAME,
      auto_name: false,
      // No sandbox: two bash scripts that echo and write their outputs need no
      // container, and an instance without Docker must still play the tour.
      sandbox: "off",
    });
    runId = created.run_id;
  }
  await waitForCompleted(runId, signal);
  prepared.runId = runId;
}

// ---- teardown ---------------------------------------------------------------

/**
 * Remove the example Trigger — every one of that name, so a Trigger a previous
 * pass left behind goes too. A 404 is a success (`deleteTrigger` says so).
 */
export async function removeExampleTrigger(): Promise<void> {
  const triggers = await fetchTriggers();
  await Promise.all(
    triggers.filter((t) => t.name === EXAMPLE_TRIGGER_NAME).map((t) => deleteTrigger(t.id)),
  );
  prepared.triggerId = null;
}

// ---- reading the observation ------------------------------------------------

const testId = (id: string) => `[data-testid="${id}"]`;

/** Attribute selectors carry ids the daemon sanitised, but a quote in one would
 *  break the selector silently rather than fail loudly. */
function cssEscape(value: string): string {
  return value.replace(/["\\]/g, "\\$&");
}

/** The three resizable panes. The panel library stamps our `id` on each. */
const CANVAS_PANEL = "[data-panel]#center";
const LEFT_PANEL = "[data-panel]#left";
const RIGHT_PANEL = "[data-panel]#right";

const RUNS_TAB = testId("left-tab-runs");
const TRIGGERS_TAB = testId("left-tab-triggers");
const PIPELINES_TAB = testId("left-tab-library");
const NEW_RUN_BUTTON = testId("new-run-button");
const TRIGGERS_LIST = testId("triggers-list-panel");
const NEW_PIPELINE_BUTTON = testId("new-pipeline-button");
const PIPELINE_ROW = testId(`library-row-${TUTORIAL_OVERVIEW_PIPELINE_ID}`);
const INSPECTOR_RUN = testId("inspector-pane-run");
const TERMINAL = testId("tmux-terminal");
const CODE_ROW = `${testId("port-row")}[data-kind="output"][data-port="code"]`;
const START_INSPECTOR = testId("start-inspector");
const EDGE_PANEL = testId("edge-detail-panel");
const CLEANUP_MODAL = testId("cleanup-confirm-modal");
const SETTINGS_BUTTON = testId("open-settings");
const STATS_BUTTON = testId("open-stats");

function runRow(id: string): string {
  return `[data-run-row="${cssEscape(id)}"]`;
}

function triggerRow(id: string): string {
  return `[data-trigger-row="${cssEscape(id)}"]`;
}

/** The tour's Run, as the list shows it, or `null` before the preparation answered. */
function tourRun(app: TourAppState): TourRunSummary | null {
  const id = overviewRunId();
  return id ? (app.runs.find((r) => r.id === id) ?? null) : null;
}

/** The canvas is showing the tour's Run — which is what clicking its row does. */
function runIsOpen(app: TourAppState): boolean {
  const id = overviewRunId();
  return id != null && app.activeRunId === id;
}

/** The open Run's pipeline, and only while it is the tour's Run. */
function runNodes(app: TourAppState): NodeDef[] {
  return runIsOpen(app) ? (app.pipeline?.nodes ?? []) : [];
}

/** The two working nodes, by id when the pipeline is ours, else in document
 *  order — a user who edited `tutorial-overview` keeps a working tour. */
function worker(app: TourAppState, id: "implementer" | "reviewer"): NodeDef | null {
  const nodes = runNodes(app);
  const byId = nodes.find((n) => n.id === id);
  if (byId) return byId;
  const workers = nodes.filter((n) => n.type !== "start" && n.type !== "end");
  return workers[id === "implementer" ? 0 : 1] ?? null;
}

function marker(app: TourAppState, type: "start" | "end"): NodeDef | null {
  return runNodes(app).find((n) => n.type === type) ?? null;
}

function nodeSel(node: NodeDef | null): string[] {
  return node ? [`.react-flow__node[data-id="${cssEscape(node.id)}"]`] : [];
}

function selectedNode(app: TourAppState, node: NodeDef | null): boolean {
  return node != null && app.selection.kind === "node" && app.selection.id === node.id;
}

/** reviewer → End — the edge the Run actually took. Its index is its canvas key. */
function endEdgeIndex(app: TourAppState): number {
  const reviewer = worker(app, "reviewer");
  const end = marker(app, "end");
  if (!reviewer || !end || !app.pipeline) return -1;
  return app.pipeline.edges.findIndex((e) => e.source.node === reviewer.id && e.target.node === end.id);
}

function isArchived(app: TourAppState): boolean {
  return tourRun(app)?.status === "archived";
}

/**
 * How long a step waits for its target. The first ones land right after a fetch
 * the click itself triggered — the Run's pipeline, the node's detail — and a
 * tour that gave up while the app was still loading would blame the reader for a
 * round trip.
 */
const READING_TIMEOUT_MS = 15_000;

/** A step that shows one thing and is acknowledged with Next. */
function readStep(def: {
  id: string;
  title: string;
  body: string;
  target: (o: TourObservation) => string[];
  waitingFor: string;
  soft?: boolean;
  failureHint?: string;
}): TourStep {
  return { ...def, targetTimeoutMs: READING_TIMEOUT_MS };
}

// ---- the steps ------------------------------------------------------------

const STEPS: TourStep[] = [
  {
    id: "open-run",
    title: "Open your Run",
    body: (o) =>
      runIsOpen(o.app)
        ? `${OVERVIEW_RUN_NAME} is open. Every Run is a row in this list; clicking one opens it on the canvas.`
        : `Click ${OVERVIEW_RUN_NAME} in the list. The tour just ran it for you: a real Run of the ${TUTORIAL_OVERVIEW_PIPELINE_ID} pipeline, already finished.`,
    // The row only exists on the Runs tab: a reader who is elsewhere is sent there.
    target: (o) => {
      const id = overviewRunId();
      if (!id) return [];
      return o.present(runRow(id)) ? [runRow(id)] : [RUNS_TAB];
    },
    waitingFor: "your Run in the list on the left",
    failureHint: "The list may be filtered — clear the filters above it.",
    targetTimeoutMs: READING_TIMEOUT_MS,
    advanceHint: "advances when the Run opens",
    done: (o) => runIsOpen(o.app),
  },
  readStep({
    id: "canvas",
    title: "This is the canvas",
    body: "A pipeline is a succession of nodes linked by edges, drawn here. This one hands a task from Start to implementer, then reviewer, then End.",
    target: () => [CANVAS_PANEL],
    soft: true,
    waitingFor: "the canvas",
  }),
  readStep({
    id: "left-panel",
    title: "The left panel",
    body: "Your Runs live here, newest first, next to the Triggers and the Pipelines tabs. Clicking a row opens it on the canvas.",
    target: () => [LEFT_PANEL],
    waitingFor: "the left panel",
  }),
  readStep({
    id: "right-panel",
    title: "The right panel",
    body: "The details of whatever is selected: the Run, a node, an edge. It is where most of the work happens.",
    target: () => [RIGHT_PANEL],
    waitingFor: "the right panel",
  }),
  {
    id: "open-implementer",
    title: "Click implementer",
    body: (o) =>
      selectedNode(o.app, worker(o.app, "implementer"))
        ? "A node is usually an agent running in a harness (Claude Code, Copilot, …); here it is a script, so the tour costs nothing. It takes an input and produces outputs."
        : "Click the implementer card. A node is usually an agent running in a harness (Claude Code, Copilot, …); this one is a script, so the tour costs nothing.",
    target: (o) => nodeSel(worker(o.app, "implementer")),
    waitingFor: "the implementer node on the canvas",
    failureHint: "It sits right under Start; scroll or zoom the canvas to bring it into view.",
    targetTimeoutMs: READING_TIMEOUT_MS,
    advanceHint: "advances when the inspector shows the node",
    done: (o) => selectedNode(o.app, worker(o.app, "implementer")) && o.present(INSPECTOR_RUN),
  },
  readStep({
    id: "outputs",
    title: "Its outputs",
    body: "A finished node's outputs are listed here: implementer wrote code, the file reviewer received. Click one any time to read it.",
    target: () => [CODE_ROW],
    waitingFor: "the outputs of implementer",
    failureHint: "They are in the node's inspector on the right, under Outputs.",
  }),
  readStep({
    id: "terminal",
    title: "Its terminal",
    body: "Every node runs in its own terminal; this one is frozen on the lines the script printed. While a node runs it is live and interactive — you talk to the harness there, and First run shows you how.",
    target: () => [TERMINAL],
    waitingFor: "the node's terminal",
    failureHint: "The terminal is in the Run tab of the node's inspector.",
  }),
  {
    id: "open-end-edge",
    title: "Click the edge reviewer → End",
    body: (o) =>
      o.app.selection.kind === "edge" && o.app.selection.edgeIndex === endEdgeIndex(o.app)
        ? "An edge carries conditions on its source's outputs and fires when the source finishes. This one reads verdict eq pass; the loop back to implementer reads verdict eq fail."
        : "Click the edge from reviewer down to End. An edge carries conditions on its source's outputs, and fires when the source finishes.",
    target: (o) => {
      const i = endEdgeIndex(o.app);
      return i < 0 ? [] : [`.react-flow__edge[data-id="e-${i}"]`];
    },
    waitingFor: "the reviewer → End edge",
    targetTimeoutMs: READING_TIMEOUT_MS,
    advanceHint: "advances when the edge is selected",
    done: (o) => {
      const i = endEdgeIndex(o.app);
      return i >= 0 && o.app.selection.kind === "edge" && o.app.selection.edgeIndex === i && o.present(EDGE_PANEL);
    },
  },
  {
    id: "open-start",
    title: "Click Start",
    body: (o) =>
      selectedNode(o.app, marker(o.app, "start"))
        ? "Start and End are special nodes: Start carries the Run's prompt, shown here, and End is where a Run stops."
        : "Click Start at the top. Start and End are special nodes: Start carries the Run's prompt, End is where a Run stops.",
    target: (o) => nodeSel(marker(o.app, "start")),
    waitingFor: "the Start node on the canvas",
    targetTimeoutMs: READING_TIMEOUT_MS,
    advanceHint: "advances when Start is selected",
    done: (o) => selectedNode(o.app, marker(o.app, "start")) && o.present(START_INSPECTOR),
  },
  readStep({
    id: "runs-tab",
    title: "The Runs tab",
    body: "Sort and filter your Runs here, and start a new one with New Run. The First run tour fills that form with you.",
    target: (o) => (o.present(NEW_RUN_BUTTON) ? [RUNS_TAB, NEW_RUN_BUTTON] : [RUNS_TAB]),
    waitingFor: "the Runs tab",
  }),
  readStep({
    id: "status-dots",
    title: "The status dot",
    body: "Green means finished. Orange means the Run waits for you, blue means it is running.",
    target: () => {
      const id = overviewRunId();
      return id ? [`${runRow(id)} ${testId("run-status-dot")}`] : [];
    },
    waitingFor: "the status dot of your Run",
    failureHint: "It is on your Run's row, in the Runs tab.",
  }),
  readStep({
    id: "worktree",
    title: "Each Run has its worktree",
    body: "A Run works in its own git worktree of the repository, which stays on disk until you clean it up. Nodes must clean up whatever they start, too.",
    target: () => {
      const id = overviewRunId();
      return id ? [runRow(id)] : [];
    },
    waitingFor: "your Run in the list",
  }),
  {
    id: "archive-run",
    title: "Archive this Run",
    body: (o) => {
      if (isArchived(o.app)) {
        return "Archived: the worktree is gone and the dot turned grey. The outputs are still there to read.";
      }
      return o.present(CLEANUP_MODAL)
        ? "Cleanup removes the Run's worktree and keeps its history and outputs. Skip leaves the Run as it is."
        : "Hover your Run's row and click its trash icon, then confirm. Archiving removes the worktree; the Run and its outputs stay.";
    },
    // Two beats, one card: the row, then the confirmation it opens — re-aimed so
    // the dialog's buttons are inside the hole rather than under the blocker.
    target: (o) => {
      if (o.present(CLEANUP_MODAL)) return [CLEANUP_MODAL];
      const id = overviewRunId();
      return id ? [runRow(id)] : [];
    },
    waitingFor: "your Run in the list",
    failureHint: "An archived Run moves to the Archived section at the bottom of the list.",
    targetTimeoutMs: READING_TIMEOUT_MS,
    skippable: true,
    advanceHint: "advances once the Run is archived",
    // Once archived the card stops for a Next: the grey dot is the thing to see.
    confirm: (o) => isArchived(o.app),
    done: (o) => isArchived(o.app),
  },
  {
    id: "triggers-tab",
    title: "Open the Triggers tab",
    body: (o) =>
      o.present(TRIGGERS_LIST)
        ? `This is ${EXAMPLE_TRIGGER_NAME}, set up for the tour: every 15 minutes it would launch ${TUTORIAL_OVERVIEW_PIPELINE_ID}, but its guard always says no. A Trigger launches a pipeline on a schedule, when its guard agrees.`
        : "Click Triggers. A Trigger launches a pipeline on a schedule, when a condition holds.",
    // Re-aims onto the example row once the list is up, the way `open-output`
    // re-aims onto the file it opened.
    target: (o) => {
      if (!o.present(TRIGGERS_LIST)) return [TRIGGERS_TAB];
      const id = exampleTriggerId();
      return id && o.present(triggerRow(id)) ? [triggerRow(id)] : [TRIGGERS_LIST];
    },
    waitingFor: "the Triggers tab",
    targetTimeoutMs: READING_TIMEOUT_MS,
    advanceHint: "advances when the tab opens",
    confirm: (o) => o.present(TRIGGERS_LIST),
    done: (o) => o.present(TRIGGERS_LIST),
  },
  {
    id: "pipelines-tab",
    title: "Open the Pipelines tab",
    body: (o) =>
      o.present(NEW_PIPELINE_BUTTON)
        ? `Pipelines are declared and edited here; ${TUTORIAL_OVERVIEW_PIPELINE_ID} is the one you just toured. The First pipeline tour builds one with you.`
        : "Click Pipelines. It is where pipelines are declared and edited.",
    target: (o) => {
      if (!o.present(NEW_PIPELINE_BUTTON)) return [PIPELINES_TAB];
      return o.present(PIPELINE_ROW) ? [PIPELINE_ROW] : [NEW_PIPELINE_BUTTON];
    },
    waitingFor: "the Pipelines tab",
    targetTimeoutMs: READING_TIMEOUT_MS,
    advanceHint: "advances when the tab opens",
    confirm: (o) => o.present(NEW_PIPELINE_BUTTON),
    done: (o) => o.present(NEW_PIPELINE_BUTTON),
  },
  readStep({
    id: "settings-button",
    title: "Settings",
    body: "Harnesses, agent profiles, skills and these tutorials live behind this button. No need to open it now: a tour of its own will come.",
    target: () => [SETTINGS_BUTTON],
    waitingFor: "the Settings button",
  }),
  readStep({
    id: "stats-button",
    title: "Stats",
    body: "What your Runs cost and how long they took, by pipeline, node and model. The tour's Run counts there too.",
    target: () => [STATS_BUTTON],
    waitingFor: "the Stats button",
  }),
];

export const OVERVIEW_TOUR: TourDef = {
  id: "overview",
  title: "Overview",
  blurb: "A walk around the screen on a real finished Run: canvas, panels, nodes, edges, Triggers. No agent needed.",
  minutes: 4,
  steps: STEPS,
  intro: {
    title: "Tour of PDO",
    body: "A walk around the screen on a real Run: the canvas, the panels, a node, an edge, the Runs, Triggers and Pipelines tabs. The tour prepares everything first; nothing needs an agent.",
    footnote:
      "Nothing here touches your own repositories. The example Trigger is removed when the tour ends; the pipeline and the Run stay yours.",
    prepare: [
      {
        id: "repo",
        pending: `Training repository ${TUTORIAL_REPO_PATH}…`,
        ready: `Training repository ${TUTORIAL_REPO_PATH} ready`,
        failureTitle: "The training repository could not be created",
        run: prepareRepoStep,
      },
      {
        id: "pipeline",
        pending: `Pipeline ${TUTORIAL_OVERVIEW_PIPELINE_ID} in the Library…`,
        ready: `Pipeline ${TUTORIAL_OVERVIEW_PIPELINE_ID} in the Library`,
        failureTitle: "The tour's pipeline could not be created",
        run: preparePipelineStep,
      },
      {
        id: "trigger",
        pending: `Example Trigger ${EXAMPLE_TRIGGER_NAME}…`,
        ready: `Example Trigger ${EXAMPLE_TRIGGER_NAME} active (it never fires a Run)`,
        failureTitle: "The example Trigger could not be created",
        run: prepareExampleTrigger,
      },
      {
        id: "run",
        pending: `Run ${OVERVIEW_RUN_NAME} running…`,
        ready: `Run ${OVERVIEW_RUN_NAME} completed`,
        failureTitle: "The tour's Run did not complete",
        run: prepareCompletedRun,
      },
    ],
  },
  teardown: [
    {
      id: "example-trigger",
      failureTitle: `The example Trigger ${EXAMPLE_TRIGGER_NAME} could not be removed`,
      run: removeExampleTrigger,
    },
  ],
  recapIntro: `You toured PDO on a real Run of ${TUTORIAL_OVERVIEW_PIPELINE_ID}:`,
  recap: (app) => [
    { label: "Canvas", text: "a pipeline is nodes linked by edges; the panels around it list and detail" },
    { label: "Nodes", text: "an input in, outputs out, a terminal of their own; usually agents, scripts here" },
    { label: "Edges", text: "conditions on the source's outputs, like `verdict eq pass`" },
    {
      label: OVERVIEW_RUN_NAME,
      text: isArchived(app)
        ? "archived: its worktree is gone, its outputs stay"
        : "still in your list, with its worktree — archive it when you are done",
    },
    { label: EXAMPLE_TRIGGER_NAME, text: "removed now that the tour is over" },
  ],
  outro: {
    closing:
      "The pipeline and the Run stay yours. First run launches a Run with a real agent; First pipeline builds a pipeline with you.",
  },
};
