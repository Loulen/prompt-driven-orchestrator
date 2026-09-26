/// <reference types="node" />
/**
 * The *Overview* tour (#911, spec #910).
 *
 * The **walk** plays the seventeen steps against a simulated app: the reader
 * does exactly the gesture each step asks for, and the machine must reach
 * `finished` in the spec's order. Then the cases the walk cannot express — the
 * gestures that must NOT advance before the state says so, the targets that
 * re-aim — and the preparations: idempotent (the Run reused or relaunched, the
 * Trigger picked up by its name), refused on a Run that did not complete.
 */
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  confirmStep,
  currentStep,
  needsConfirm,
  observeTour,
  recapOf,
  skipStep,
  startTour,
  beginSteps,
  stepBody,
  stepSoft,
  type TourAppState,
  type TourObservation,
  type TourRun,
  type TourRunSummary,
} from "../tour";
import type { PipelineDef, RunListEntry, RunState, Trigger } from "../../types";
import {
  EXAMPLE_TRIGGER_NAME,
  IMPLEMENTER_SCRIPT,
  OVERVIEW_PIPELINE_YAML,
  OVERVIEW_PROMPT,
  OVERVIEW_RUN_NAME,
  OVERVIEW_TOUR,
  REVIEWER_SCRIPT,
  RUN_POLL_MS,
  TUTORIAL_OVERVIEW_PIPELINE_ID,
  exampleTriggerId,
  overviewRunId,
  prepareCompletedRun,
  prepareExampleTrigger,
  removeExampleTrigger,
} from "./overview";
import { TUTORIAL_REPO_PATH } from "./firstRun";
import { useEditStore } from "../../stores/editStore";

vi.mock("../../api", async () => {
  const actual = await vi.importActual<typeof import("../../api")>("../../api");
  return {
    ...actual,
    createRepo: vi.fn(async () => ({ path: "/tmp/pdo-tutorial", created: true })),
    fetchPipelines: vi.fn(async () => [] as unknown[]),
    createPipeline: vi.fn(async () => ({ id: "tutorial-overview", scope: "instance", path: "p" })),
    savePipeline: vi.fn(async () => ({ ok: true })),
    fetchRuns: vi.fn(async () => [] as unknown[]),
    fetchRun: vi.fn(),
    createRun: vi.fn(async () => ({ run_id: "run-new" })),
    fetchTriggers: vi.fn(async () => [] as unknown[]),
    createTrigger: vi.fn(async () => ({ id: "trg-new" })),
    updateTrigger: vi.fn(async () => ({})),
    deleteTrigger: vi.fn(async () => undefined),
  };
});

const api = await import("../../api");

const TOUR = OVERVIEW_TOUR;
const STEPS = TOUR.steps;
const RUN_ID = "run-tour";
const TRIGGER_ID = "trg-tour";

const t = (id: string) => `[data-testid="${id}"]`;
const RUN_ROW = `[data-run-row="${RUN_ID}"]`;
const RUN_DOT = `${RUN_ROW} ${t("run-status-dot")}`;
const TRIGGER_ROW = `[data-trigger-row="${TRIGGER_ID}"]`;
const CANVAS = "[data-panel]#center";
const LEFT = "[data-panel]#left";
const RIGHT = "[data-panel]#right";
const RUNS_TAB = t("left-tab-runs");
const TRIGGERS_TAB = t("left-tab-triggers");
const PIPELINES_TAB = t("left-tab-library");
const NEW_RUN = t("new-run-button");
const TRIGGERS_LIST = t("triggers-list-panel");
const NEW_PIPELINE = t("new-pipeline-button");
const PIPELINE_ROW = t("library-row-tutorial-overview");
const INSPECTOR_RUN = t("inspector-pane-run");
const TERMINAL = t("tmux-terminal");
const TERMINAL_RESTORE = t("term-restore");
const RUNS_TAB_OPEN = `${RUNS_TAB}[aria-selected="true"]`;
const CODE_ROW = `${t("port-row")}[data-kind="output"][data-port="code"]`;
const START_INSPECTOR = t("start-inspector");
const EDGE_PANEL = t("edge-detail-panel");
const CLEANUP_MODAL = t("cleanup-confirm-modal");
const node = (id: string) => `.react-flow__node[data-id="${id}"]`;

/** The canvas document of the Run: the pipeline the preparation wrote. */
const RUN_PIPELINE: PipelineDef = {
  name: TUTORIAL_OVERVIEW_PIPELINE_ID,
  nodes: [
    { id: "start", type: "start", outputs: [] },
    { id: "end", type: "end", outputs: [] },
    { id: "implementer", type: "script", outputs: [] },
    { id: "reviewer", type: "script", outputs: [] },
  ],
  edges: [
    { source: { node: "start", port: "user_prompt" }, target: { node: "implementer", port: "task" } },
    { source: { node: "implementer", port: "code" }, target: { node: "reviewer", port: "code" } },
    { source: { node: "reviewer", port: "review" }, target: { node: "implementer", port: "task" } },
    { source: { node: "reviewer", port: "review" }, target: { node: "end", port: "result" } },
  ],
} as unknown as PipelineDef;
/** reviewer → End is the 4th edge of the document: the canvas keys it `e-3`. */
const END_EDGE = `.react-flow__edge[data-id="e-3"]`;
const END_PILL = t("edge-condition-label-e-3");

const EMPTY_APP: TourAppState = {
  pipelineId: null,
  pipeline: null,
  prompts: {},
  selection: { kind: "none", id: null, edgeIndex: null },
  dirty: false,
  libraryPipelineIds: [],
  runCount: 0,
  runs: [],
  latestRun: null,
  activeRunId: null,
};

function summary(over: Partial<TourRunSummary> = {}): TourRunSummary {
  return { id: RUN_ID, name: OVERVIEW_RUN_NAME, pipeline: TUTORIAL_OVERVIEW_PIPELINE_ID, status: "completed", ...over };
}

function listEntry(over: Partial<RunListEntry> = {}): RunListEntry {
  return {
    run_id: RUN_ID,
    pipeline_name: TUTORIAL_OVERVIEW_PIPELINE_ID,
    status: "completed",
    started_at: null,
    name: OVERVIEW_RUN_NAME,
    ...over,
  } as RunListEntry;
}

function runState(over: Partial<RunState> = {}): RunState {
  return { run_id: RUN_ID, status: "completed", name: OVERVIEW_RUN_NAME, ...over } as RunState;
}

/** Make the preparations answer with the tour's Run and Trigger, as a real pass would. */
async function prepare(): Promise<void> {
  vi.mocked(api.fetchRuns).mockResolvedValueOnce([listEntry()]);
  vi.mocked(api.fetchRun).mockResolvedValueOnce(runState());
  await prepareCompletedRun();
  vi.mocked(api.fetchTriggers).mockResolvedValueOnce([
    { id: TRIGGER_ID, name: EXAMPLE_TRIGGER_NAME, enabled: true, guard_command: "false" } as Trigger,
  ]);
  await prepareExampleTrigger();
}

// ---- the simulator --------------------------------------------------------

/**
 * The app around the tour: which elements are on screen and what is selected.
 * It starts on the Runs tab, with the tour's Run in the list — and a newer Run of
 * somebody else's on top, so a step that followed « the newest Run » instead of
 * the tour's would be caught.
 */
class FakeApp {
  app: TourAppState = {
    ...EMPTY_APP,
    runCount: 2,
    runs: [summary({ id: "run-other", name: "other", pipeline: "mine", status: "running" }), summary()],
    latestRun: { id: "run-other", name: "other", nodes: [] },
  };
  readonly baseline: TourAppState = { ...this.app };
  private dom = new Set<string>([CANVAS, LEFT, RIGHT, RUNS_TAB, RUNS_TAB_OPEN, TRIGGERS_TAB, PIPELINES_TAB, NEW_RUN, RUN_ROW, RUN_DOT, t("open-settings"), t("open-stats")]);

  obs(): TourObservation {
    return {
      app: this.app,
      baseline: this.baseline,
      present: (selector) => this.dom.has(selector),
      value: () => null,
      text: () => null,
    };
  }

  show(...selectors: string[]) {
    for (const s of selectors) this.dom.add(s);
  }
  hide(...selectors: string[]) {
    for (const s of selectors) this.dom.delete(s);
  }
  set(patch: Partial<TourAppState>) {
    this.app = { ...this.app, ...patch };
  }
  select(kind: string, id: string | null, edgeIndex: number | null = null) {
    this.set({ selection: { kind, id, edgeIndex } });
  }

  // The gestures the steps ask for, the way the real UI answers them.
  openRun() {
    this.set({ activeRunId: RUN_ID, pipelineId: `run:${RUN_ID}`, pipeline: RUN_PIPELINE });
    this.show(node("start"), node("implementer"), node("reviewer"), node("end"), END_EDGE, END_PILL);
  }
  /** A finished node opens with its terminal folded to a bar (#346). */
  clickImplementer() {
    this.select("node", "implementer");
    this.show(INSPECTOR_RUN, CODE_ROW, TERMINAL_RESTORE);
  }
  unfoldTerminal() {
    this.hide(TERMINAL_RESTORE);
    this.show(TERMINAL);
  }
  clickEndEdge() {
    this.select("edge", null, 3);
    this.hide(INSPECTOR_RUN, CODE_ROW, TERMINAL, TERMINAL_RESTORE);
    this.show(EDGE_PANEL);
  }
  clickStart() {
    this.select("node", "start");
    this.hide(EDGE_PANEL);
    this.show(START_INSPECTOR);
  }
  archive() {
    this.show(CLEANUP_MODAL);
  }
  confirmArchive() {
    this.hide(CLEANUP_MODAL);
    this.set({ runs: this.app.runs.map((r) => (r.id === RUN_ID ? { ...r, status: "archived" } : r)) });
  }
  openRunsTab() {
    this.hide(TRIGGERS_LIST, TRIGGER_ROW, NEW_PIPELINE, PIPELINE_ROW);
    this.show(RUNS_TAB_OPEN, NEW_RUN, RUN_ROW, RUN_DOT);
  }
  openTriggers() {
    this.hide(RUNS_TAB_OPEN, NEW_RUN, RUN_ROW, RUN_DOT);
    this.show(TRIGGERS_LIST, TRIGGER_ROW);
  }
  openPipelines() {
    this.hide(TRIGGERS_LIST, TRIGGER_ROW);
    this.show(NEW_PIPELINE, PIPELINE_ROW);
  }
}

/** What the reader does on each step. Acknowledge steps need nothing: Next. */
function gestures(fake: FakeApp): Record<string, () => void> {
  const read = () => {};
  return {
    "open-run": () => fake.openRun(),
    canvas: read,
    "left-panel": read,
    "right-panel": read,
    "open-implementer": () => fake.clickImplementer(),
    outputs: read,
    terminal: () => fake.unfoldTerminal(),
    "open-end-edge": () => fake.clickEndEdge(),
    "open-start": () => fake.clickStart(),
    "runs-tab": read,
    "status-dots": read,
    worktree: read,
    "archive-run": () => {
      fake.archive();
      fake.confirmArchive();
    },
    "triggers-tab": () => fake.openTriggers(),
    "pipelines-tab": () => fake.openPipelines(),
    "settings-button": read,
    "stats-button": read,
  };
}

/** Play the tour: on each step, check its target is there, do the gesture, then
 *  let the machine observe and press Next when the card asks for it. */
function walk(fake: FakeApp): { run: TourRun; visited: string[] } {
  const script = gestures(fake);
  let run = beginSteps(TOUR, startTour(TOUR, fake.obs()), fake.obs());
  const visited: string[] = [];
  let now = 0;
  while (run.phase === "running") {
    const step = currentStep(TOUR, run)!;
    visited.push(step.id);
    const targets = step.target(fake.obs());
    expect(targets.length, `${step.id} has a target`).toBeGreaterThan(0);
    for (const s of targets) expect(fake.obs().present(s), `${step.id} target ${s}`).toBe(true);
    script[step.id]();
    now += 100;
    run = observeTour(TOUR, run, fake.obs(), now);
    if (run.phase !== "running") break;
    if (currentStep(TOUR, run)?.id === step.id) {
      expect(needsConfirm(TOUR, run, fake.obs()), `${step.id} is waiting for Next`).toBe(true);
      const next = confirmStep(TOUR, run, fake.obs());
      expect(next, `${step.id} Next is enabled`).not.toBe(run);
      run = next;
    }
  }
  return { run, visited };
}

beforeEach(async () => {
  vi.clearAllMocks();
  await prepare();
});

const stepById = (id: string) => STEPS.find((s) => s.id === id)!;

describe("walking the whole tour", () => {
  it("reaches the end in the spec's order, on the tour's Run rather than the newest", () => {
    const fake = new FakeApp();
    const { run, visited } = walk(fake);

    expect(run.phase).toBe("finished");
    expect(visited).toEqual([
      "open-run",
      "canvas",
      "left-panel",
      "right-panel",
      "open-implementer",
      "outputs",
      "terminal",
      "open-end-edge",
      "open-start",
      "runs-tab",
      "status-dots",
      "worktree",
      "archive-run",
      "triggers-tab",
      "pipelines-tab",
      "settings-button",
      "stats-button",
    ]);
  });

  it("is seventeen steps: seven gestures, and two that ask for a click only when their subject is hidden", () => {
    expect(STEPS).toHaveLength(17);
    const gestures = STEPS.filter((s) => s.done).map((s) => s.id);
    expect(gestures).toEqual([
      "open-run",
      "open-implementer",
      "terminal",
      "open-end-edge",
      "open-start",
      "runs-tab",
      "archive-run",
      "triggers-tab",
      "pipelines-tab",
    ]);
    // Start comes after the edge: ordinary nodes first, special ones next (Q12).
    expect(STEPS.findIndex((s) => s.id === "open-start")).toBeGreaterThan(
      STEPS.findIndex((s) => s.id === "open-end-edge"),
    );
  });
});

describe("gestures advance only on the observed state", () => {
  /** The gesture is done: the card stays, now asking for Next, and Next moves on. */
  function expectStopsForNext(run: TourRun, fake: FakeApp, now: number) {
    const after = observeTour(TOUR, run, fake.obs(), now);
    expect(after.index, "stops on the satisfied body").toBe(run.index);
    expect(needsConfirm(TOUR, after, fake.obs())).toBe(true);
    expect(confirmStep(TOUR, after, fake.obs()).index).toBe(run.index + 1);
  }

  function at(id: string, fake: FakeApp): TourRun {
    let run = beginSteps(TOUR, startTour(TOUR, fake.obs()), fake.obs());
    // Entered on the state as it is now — not on step 1's, which the Run
    // already being open satisfies.
    run = { ...run, index: STEPS.findIndex((s) => s.id === id), satisfiedOnEntry: false };
    return run;
  }

  it("waits for the tour's Run to be open, not just any Run", () => {
    const fake = new FakeApp();
    const run = at("open-run", fake);
    fake.set({ activeRunId: "run-other" });
    expect(observeTour(TOUR, run, fake.obs(), 100).index).toBe(run.index);
    fake.openRun();
    expect(observeTour(TOUR, run, fake.obs(), 200).index).toBe(run.index + 1);
  });

  it("sends a reader on another tab to the Runs tab for their Run", () => {
    const fake = new FakeApp();
    fake.hide(RUN_ROW);
    expect(stepById("open-run").target(fake.obs())).toEqual([RUNS_TAB]);
    fake.show(RUN_ROW);
    expect(stepById("open-run").target(fake.obs())).toEqual([RUN_ROW]);
  });

  it("does not take reviewer for implementer", () => {
    const fake = new FakeApp();
    fake.openRun();
    const run = at("open-implementer", fake);
    fake.select("node", "reviewer");
    fake.show(INSPECTOR_RUN);
    expect(observeTour(TOUR, run, fake.obs(), 100).index).toBe(run.index);
    fake.clickImplementer();
    expectStopsForNext(run, fake, 200);
  });

  it("points at reviewer → End, and the loop edge does not count", () => {
    const fake = new FakeApp();
    fake.openRun();
    const run = at("open-end-edge", fake);
    expect(stepById("open-end-edge").target(fake.obs())).toEqual([END_EDGE, END_PILL]);
    fake.select("edge", null, 2); // reviewer → implementer, verdict eq fail
    fake.show(EDGE_PANEL);
    expect(observeTour(TOUR, run, fake.obs(), 100).index).toBe(run.index);
    fake.clickEndEdge();
    expectStopsForNext(run, fake, 200);
    expect(stepBody(stepById("open-end-edge"), fake.obs())).toContain("verdict eq pass");
  });

  it("lights the edge alone while its condition pill is not rendered", () => {
    const fake = new FakeApp();
    fake.openRun();
    fake.hide(END_PILL);
    expect(stepById("open-end-edge").target(fake.obs())).toEqual([END_EDGE]);
  });

  it("advances on Start only once its inspector shows the Run's prompt", () => {
    const fake = new FakeApp();
    fake.openRun();
    const run = at("open-start", fake);
    fake.select("node", "start");
    expect(observeTour(TOUR, run, fake.obs(), 100).index).toBe(run.index);
    fake.show(START_INSPECTOR);
    expectStopsForNext(run, fake, 200);
    expect(stepBody(stepById("open-start"), fake.obs())).toContain("shown here");
  });

  it("asks for the folded terminal to be unfolded, then stops on it for Next", () => {
    const fake = new FakeApp();
    fake.openRun();
    fake.clickImplementer();
    const step = stepById("terminal");
    const run = at("terminal", fake);
    expect(step.target(fake.obs())).toEqual([TERMINAL_RESTORE]);
    expect(stepBody(step, fake.obs())).toMatch(/^Click Terminal/);
    expect(needsConfirm(TOUR, run, fake.obs()), "no Next before the click").toBe(false);
    expect(observeTour(TOUR, run, fake.obs(), 100).index).toBe(run.index);
    fake.unfoldTerminal();
    expect(step.target(fake.obs())).toEqual([TERMINAL]);
    expectStopsForNext(run, fake, 200);
    expect(stepBody(step, fake.obs())).toContain("frozen");
  });

  it("does not stop the tour while the terminal is folded, however long the reader takes", () => {
    const fake = new FakeApp();
    fake.openRun();
    fake.clickImplementer();
    const run = at("terminal", fake);
    expect(observeTour(TOUR, run, fake.obs(), 60_000).phase).toBe("running");
  });

  it("sends a reader who left the Runs tab back to it before the steps that read the list", () => {
    const fake = new FakeApp();
    fake.openTriggers();
    const step = stepById("runs-tab");
    const run = at("runs-tab", fake);
    expect(step.target(fake.obs())).toEqual([RUNS_TAB]);
    expect(stepBody(step, fake.obs())).toMatch(/^Click Runs/);
    expect(observeTour(TOUR, run, fake.obs(), 100).index).toBe(run.index);
    fake.openRunsTab();
    expect(step.target(fake.obs())).toEqual([RUNS_TAB, NEW_RUN]);
    expectStopsForNext(run, fake, 200);
  });

  it("only asks for Next on the Runs tab when it is already open", () => {
    const fake = new FakeApp();
    const run = beginSteps(TOUR, startTour(TOUR, fake.obs()), fake.obs());
    const entered = confirmStep(TOUR, { ...run, index: STEPS.findIndex((s) => s.id === "runs-tab") - 1, satisfiedOnEntry: true }, fake.obs());
    expect(currentStep(TOUR, entered)?.id).toBe("runs-tab");
    expect(entered.satisfiedOnEntry).toBe(true);
    expect(needsConfirm(TOUR, entered, fake.obs())).toBe(true);
  });

  it("re-aims the archive step onto the confirmation, then waits for Next on the grey dot", () => {
    const fake = new FakeApp();
    const step = stepById("archive-run");
    const run = at("archive-run", fake);
    expect(step.target(fake.obs())).toEqual([RUN_ROW]);
    fake.archive();
    expect(step.target(fake.obs())).toEqual([CLEANUP_MODAL]);
    expect(observeTour(TOUR, run, fake.obs(), 100).index).toBe(run.index);
    fake.confirmArchive();
    const after = observeTour(TOUR, run, fake.obs(), 200);
    expect(after.index, "stops on the archived Run").toBe(run.index);
    expect(needsConfirm(TOUR, after, fake.obs())).toBe(true);
    expect(stepBody(step, fake.obs())).toMatch(/grey/);
  });

  it("lets the archive be skipped, leaving the Run as it is", () => {
    const fake = new FakeApp();
    const run = at("archive-run", fake);
    expect(stepById("archive-run").skippable).toBe(true);
    const skipped = skipStep(TOUR, run, fake.obs());
    expect(currentStep(TOUR, skipped)?.id).toBe("triggers-tab");
    expect(fake.app.runs.find((r) => r.id === RUN_ID)?.status).toBe("completed");
  });

  it("re-aims the Triggers step onto the example Trigger's row once the tab is open", () => {
    const fake = new FakeApp();
    const step = stepById("triggers-tab");
    expect(step.target(fake.obs())).toEqual([TRIGGERS_TAB]);
    fake.openTriggers();
    expect(step.target(fake.obs())).toEqual([TRIGGER_ROW]);
    expect(stepBody(step, fake.obs())).toContain(EXAMPLE_TRIGGER_NAME);
  });

  it("re-aims the Pipelines step onto tutorial-overview, with one sentence for First pipeline", () => {
    const fake = new FakeApp();
    const step = stepById("pipelines-tab");
    expect(step.target(fake.obs())).toEqual([PIPELINES_TAB]);
    fake.openPipelines();
    expect(step.target(fake.obs())).toEqual([PIPELINE_ROW]);
    expect(stepBody(step, fake.obs())).toContain("First pipeline");
  });

  it("lights Settings and Stats without asking for a click", () => {
    for (const id of ["settings-button", "stats-button"]) {
      const step = stepById(id);
      expect(step.done, id).toBeUndefined();
    }
    expect(stepById("settings-button").target(new FakeApp().obs())).toEqual([t("open-settings")]);
    expect(stepById("stats-button").target(new FakeApp().obs())).toEqual([t("open-stats")]);
  });

  it("draws the canvas as a zone to roam in", () => {
    expect(stepSoft(stepById("canvas"), new FakeApp().obs())).toBe(true);
  });

  it("points to First run in one sentence, from the Runs tab and the terminal", () => {
    const fake = new FakeApp();
    expect(stepBody(stepById("runs-tab"), fake.obs())).toContain("First run");
    fake.unfoldTerminal();
    expect(stepBody(stepById("terminal"), fake.obs())).toContain("First run");
  });
});

describe("the copy", () => {
  it("gives every step a title without a full stop and a short body", () => {
    const fake = new FakeApp();
    fake.openRun();
    for (const step of STEPS) {
      expect(step.title, step.id).not.toMatch(/\.$/);
      const sentences = stepBody(step, fake.obs()).split(/(?<=[.!?])\s+/).filter(Boolean);
      expect(sentences.length, step.id).toBeLessThanOrEqual(3);
      expect(step.waitingFor.length, step.id).toBeGreaterThan(0);
    }
  });

  it("recaps what happened to the Run", () => {
    const fake = new FakeApp();
    expect(recapOf(TOUR, fake.app).find((b) => b.label === OVERVIEW_RUN_NAME)?.text).toMatch(/still in your list/);
    fake.confirmArchive();
    expect(recapOf(TOUR, fake.app).find((b) => b.label === OVERVIEW_RUN_NAME)?.text).toMatch(/archived/);
  });
});

// ---- the pipeline ---------------------------------------------------------

describe("the tutorial-overview pipeline", () => {
  const here = dirname(fileURLToPath(import.meta.url));
  const fixture = readFileSync(
    resolve(here, "../../../../scripts/readme-media/fixture/targets/implement-review.yaml"),
    "utf-8",
  );
  const views = (yaml: string) =>
    [...yaml.matchAll(/- id: (\w+)[\s\S]*?view: \{ x: (-?\d+), y: (-?\d+) \}/g)].map((m) => `${m[1]}@${m[2]},${m[3]}`);
  const anchors = (yaml: string) => [...yaml.matchAll(/(source|target)_anchor: \{[^}]+\}/g)].map((m) => m[0]);

  it("takes implement-review's layout: node positions and edge anchors", () => {
    expect(views(OVERVIEW_PIPELINE_YAML)).toEqual(views(fixture));
    expect(anchors(OVERVIEW_PIPELINE_YAML)).toEqual(anchors(fixture));
  });

  it("has two script nodes and no image_list port", () => {
    expect(OVERVIEW_PIPELINE_YAML.match(/type: script/g)).toHaveLength(2);
    expect(OVERVIEW_PIPELINE_YAML).not.toContain("type: agent");
    expect(OVERVIEW_PIPELINE_YAML).not.toContain("image_list");
    expect(OVERVIEW_PIPELINE_YAML).not.toContain("pin_harness");
  });

  it("routes reviewer → End on verdict eq pass and back on verdict eq fail", () => {
    const edges = OVERVIEW_PIPELINE_YAML.split("edges:")[1];
    expect(edges).toMatch(/node: reviewer, port: review \}\n\s+target: \{ node: end[\s\S]*?verdict: \{ eq: pass \}/);
    expect(edges).toMatch(/node: reviewer, port: review \}\n\s+target: \{ node: implementer[\s\S]*?verdict: \{ eq: fail \}/);
  });

  it("writes readable lines and valid outputs, and never commits", () => {
    for (const body of [IMPLEMENTER_SCRIPT, REVIEWER_SCRIPT]) {
      expect(body).toMatch(/^#!\/usr\/bin\/env bash/);
      expect(body).toContain("set -euo pipefail");
      expect(body).toMatch(/echo "/);
      expect(body).not.toMatch(/git (add|commit)/);
      // It completes from inside its session, then waits for the reap, so the
      // daemon's pane snapshot still has the lines to freeze — and it leaves
      // on a poll of its own pane rather than a sleep that would outlive it.
      expect(body).toMatch(/\npdo complete\n/);
      expect(body).toContain('tmux display-message -p -t "${TMUX_PANE:-}"');
      expect(body).not.toMatch(/\n(exec )?sleep \d{2,}/);
    }
    expect(IMPLEMENTER_SCRIPT).toContain('> "$PDO_OUTPUT_CODE"');
    expect(REVIEWER_SCRIPT).toContain('> "$PDO_OUTPUT_REVIEW"');
    expect(REVIEWER_SCRIPT).toMatch(/---\nverdict: pass\n---/);
    expect(REVIEWER_SCRIPT).toContain("```mermaid");
  });

  it("is written once, with both script bodies, and left alone when it exists", async () => {
    const prepareStep = TOUR.intro!.prepare.find((p) => p.id === "pipeline")!;
    vi.mocked(api.fetchPipelines).mockResolvedValueOnce([]);
    await prepareStep.run();
    expect(api.createPipeline).toHaveBeenCalledWith(TUTORIAL_OVERVIEW_PIPELINE_ID);
    const [, yaml, prompts] = vi.mocked(api.savePipeline).mock.calls.at(-1)!;
    expect(yaml).toBe(OVERVIEW_PIPELINE_YAML);
    expect(prompts).toEqual({ implementer: IMPLEMENTER_SCRIPT, reviewer: REVIEWER_SCRIPT });

    vi.mocked(api.createPipeline).mockClear();
    vi.mocked(api.savePipeline).mockClear();
    vi.mocked(api.fetchPipelines).mockResolvedValueOnce([{ id: TUTORIAL_OVERVIEW_PIPELINE_ID } as never]);
    await prepareStep.run();
    expect(api.createPipeline).not.toHaveBeenCalled();
    expect(api.savePipeline).not.toHaveBeenCalled();
  });

  it("makes the Library the left panel shows re-read the list, so the Pipelines step finds it", async () => {
    const prepareStep = TOUR.intro!.prepare.find((p) => p.id === "pipeline")!;
    useEditStore.setState({ pipelines: [] });
    vi.mocked(api.fetchPipelines)
      .mockResolvedValueOnce([])
      .mockResolvedValueOnce([{ id: TUTORIAL_OVERVIEW_PIPELINE_ID } as never]);
    await prepareStep.run();
    expect(useEditStore.getState().pipelines.map((p) => p.id)).toEqual([TUTORIAL_OVERVIEW_PIPELINE_ID]);
  });
});

// ---- the preparations -----------------------------------------------------

describe("the Run the preparation waits for", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("reuses the newest completed, unarchived tutorial-overview Run", async () => {
    vi.mocked(api.fetchRuns).mockResolvedValueOnce([
      listEntry({ run_id: "run-archived", status: "archived" }),
      listEntry({ run_id: "run-mine", pipeline_name: "mine" }),
      listEntry({ run_id: "run-done" }),
      listEntry({ run_id: "run-older" }),
    ]);
    vi.mocked(api.fetchRun).mockResolvedValueOnce(runState({ run_id: "run-done" }));

    await prepareCompletedRun();

    expect(api.createRun).not.toHaveBeenCalled();
    expect(overviewRunId()).toBe("run-done");
  });

  it("launches a new Run once the last one was archived, and answers when it completes", async () => {
    vi.useFakeTimers();
    vi.mocked(api.fetchRuns).mockResolvedValueOnce([listEntry({ status: "archived" })]);
    vi.mocked(api.fetchRun)
      .mockResolvedValueOnce(runState({ run_id: "run-new", status: "running" }))
      .mockResolvedValueOnce(runState({ run_id: "run-new", status: "completed" }));

    let answered = false;
    const pending = prepareCompletedRun().then(() => {
      answered = true;
    });
    await vi.advanceTimersByTimeAsync(0);
    expect(answered, "not while the Run is still running").toBe(false);
    await vi.advanceTimersByTimeAsync(RUN_POLL_MS);
    await pending;

    expect(api.createRun).toHaveBeenCalledWith(
      expect.objectContaining({
        pipeline: TUTORIAL_OVERVIEW_PIPELINE_ID,
        pipeline_id: TUTORIAL_OVERVIEW_PIPELINE_ID,
        input: OVERVIEW_PROMPT,
        target_repo: TUTORIAL_REPO_PATH,
        name: OVERVIEW_RUN_NAME,
        auto_name: false,
      }),
    );
    // The repository and the pipeline exist before the Run is asked for.
    expect(api.createRepo).toHaveBeenCalled();
    expect(overviewRunId()).toBe("run-new");
  });

  it("refuses a Run that ended anywhere but completed, quoting the reason", async () => {
    vi.mocked(api.fetchRuns).mockResolvedValueOnce([]);
    vi.mocked(api.fetchRun).mockResolvedValueOnce(
      runState({ run_id: "run-new", status: "failed", failure_reason: "node reviewer failed: missing output review" }),
    );

    await expect(prepareCompletedRun()).rejects.toThrow(/ended `failed`.*missing output review/);
    expect(overviewRunId()).toBeNull();
  });

  it("refuses a Run parked awaiting a human", async () => {
    vi.mocked(api.fetchRuns).mockResolvedValueOnce([]);
    vi.mocked(api.fetchRun).mockResolvedValueOnce(
      runState({ run_id: "run-new", status: "awaiting_user", awaiting_reason: "sandbox unavailable" }),
    );
    await expect(prepareCompletedRun()).rejects.toThrow(/awaiting_user.*sandbox unavailable/);
  });

  it("lets a daemon refusal at launch through, so the card can quote it", async () => {
    vi.mocked(api.fetchRuns).mockResolvedValueOnce([]);
    vi.mocked(api.createRun).mockRejectedValueOnce(new Error("cannot launch run: script node(s) have an empty body: reviewer"));
    await expect(prepareCompletedRun()).rejects.toThrow("empty body");
  });

  it("stops polling once the card is gone", async () => {
    vi.useFakeTimers();
    vi.mocked(api.fetchRuns).mockResolvedValueOnce([listEntry({ status: "running" })]);
    vi.mocked(api.fetchRun).mockResolvedValue(runState({ status: "running" }));
    const abort = new AbortController();
    const pending = prepareCompletedRun(abort.signal);
    const settled = expect(pending).rejects.toThrow(/closed/);
    await vi.advanceTimersByTimeAsync(0);
    abort.abort();
    await settled;
    const calls = vi.mocked(api.fetchRun).mock.calls.length;
    await vi.advanceTimersByTimeAsync(RUN_POLL_MS * 5);
    expect(vi.mocked(api.fetchRun).mock.calls.length).toBe(calls);
    expect(api.createRun, "a running tour Run is waited for, not doubled").not.toHaveBeenCalled();
  });
});

describe("the example Trigger", () => {
  it("is created active, on the tour's pipeline, with a guard that always refuses", async () => {
    vi.mocked(api.fetchTriggers).mockResolvedValueOnce([]);
    await prepareExampleTrigger();

    expect(api.createTrigger).toHaveBeenCalledWith(
      expect.objectContaining({
        name: EXAMPLE_TRIGGER_NAME,
        pipeline_id: TUTORIAL_OVERVIEW_PIPELINE_ID,
        target_repo: TUTORIAL_REPO_PATH,
        cron: "*/15 * * * *",
        guard_command: "false",
      }),
    );
    // Not sent disabled: the daemon creates a Trigger active.
    expect(vi.mocked(api.createTrigger).mock.calls[0][0]).not.toHaveProperty("enabled");
    expect(exampleTriggerId()).toBe("trg-new");
  });

  it("is picked up by its name when a previous pass left it behind", async () => {
    vi.mocked(api.fetchTriggers).mockResolvedValueOnce([
      { id: "trg-left", name: EXAMPLE_TRIGGER_NAME, enabled: true, guard_command: "false" } as Trigger,
    ]);
    await prepareExampleTrigger();
    expect(api.createTrigger).not.toHaveBeenCalled();
    expect(api.updateTrigger).not.toHaveBeenCalled();
    expect(exampleTriggerId()).toBe("trg-left");
  });

  it("is put back active and harmless when it was left paused or edited", async () => {
    vi.mocked(api.fetchTriggers).mockResolvedValueOnce([
      { id: "trg-left", name: EXAMPLE_TRIGGER_NAME, enabled: false, guard_command: "true" } as Trigger,
    ]);
    await prepareExampleTrigger();
    expect(api.updateTrigger).toHaveBeenCalledWith("trg-left", { enabled: true, guard_command: "false" });
  });

  it("is removed by the teardown — every Trigger of that name, and only those", async () => {
    vi.mocked(api.fetchTriggers).mockResolvedValueOnce([
      { id: "a", name: EXAMPLE_TRIGGER_NAME } as Trigger,
      { id: "b", name: "nightly" } as Trigger,
      { id: "c", name: EXAMPLE_TRIGGER_NAME } as Trigger,
    ]);
    await removeExampleTrigger();
    expect(vi.mocked(api.deleteTrigger).mock.calls.map((c) => c[0])).toEqual(["a", "c"]);
    expect(exampleTriggerId()).toBeNull();
  });

  it("is the tour's only teardown: the pipeline and the Run stay", () => {
    expect(TOUR.teardown?.map((t) => t.id)).toEqual(["example-trigger"]);
    expect(TOUR.teardown?.[0].run).toBe(removeExampleTrigger);
  });
});

describe("the intro card", () => {
  it("prepares the repository, the pipeline, the Trigger and a completed Run", () => {
    expect(TOUR.intro!.prepare.map((p) => p.id)).toEqual(["repo", "pipeline", "trigger", "run"]);
    for (const item of TOUR.intro!.prepare) {
      expect(item.failureTitle.length, item.id).toBeGreaterThan(0);
      expect(item.ready, item.id).not.toBe(item.pending);
    }
  });
});
