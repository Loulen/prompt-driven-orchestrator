/**
 * How the *Overview* tour (#911, spec #910) reads the app: the selectors its
 * steps aim at, and the predicates on the observation that say which gesture
 * has been made. The steps themselves live in `overviewCanvasSteps.ts` and
 * `overview.ts`.
 */

import type { EditorNodeDef } from "../../pipeline/model/pipeline";
import type { TourAppState, TourObservation, TourRunSummary, TourStep } from "../tourDef";
import type { TourIllustration } from "../tourPanels";
import { OVERVIEW_RUN_NAME, TUTORIAL_OVERVIEW_PIPELINE_ID } from "./overviewPipeline";
import { overviewRunId } from "./overviewPreparation";

const testId = (id: string) => `[data-testid="${id}"]`;

/** Attribute selectors carry ids the daemon sanitised, but a quote in one would
 *  break the selector silently rather than fail loudly. */
export function cssEscape(value: string): string {
  return value.replace(/["\\]/g, "\\$&");
}

/** The three resizable panes. The panel library stamps our `id` on each. */
export const CANVAS_PANEL = "[data-panel]#center";
export const LEFT_PANEL = "[data-panel]#left";
export const RIGHT_PANEL = "[data-panel]#right";

export const RUNS_TAB = testId("left-tab-runs");
export const TRIGGERS_TAB = testId("left-tab-triggers");
export const PIPELINES_TAB = testId("left-tab-library");
export const NEW_RUN_BUTTON = testId("new-run-button");
export const TRIGGERS_LIST = testId("triggers-list-panel");
export const NEW_PIPELINE_BUTTON = testId("new-pipeline-button");
export const PIPELINE_ROW = testId(`library-row-${TUTORIAL_OVERVIEW_PIPELINE_ID}`);
export const INSPECTOR_RUN = testId("inspector-pane-run");
export const INSPECTOR_EDIT = testId("inspector-pane-edit");
/** The inspector's Run tab — where a node's outputs and terminal live. */
export const INSPECTOR_TAB_RUN = testId("inspector-tab-run");
export const TERMINAL = testId("tmux-terminal");
/** A finished node's terminal opens folded to this bar, to leave room for its outputs. */
export const TERMINAL_RESTORE = testId("term-restore");
/** The Runs tab, and it is the one showing. */
export const RUNS_TAB_OPEN = `${RUNS_TAB}[aria-selected="true"]`;
export const CODE_ROW = `${testId("port-row")}[data-kind="output"][data-port="code"]`;
/** The viewer a click on the `code` row opens — the artifact modal, keyed by port. */
export const CODE_VIEWER = `${testId("artifact-modal")}[data-port="code"]`;
/** implementer's input row, where Start's prompt arrived. */
export const TASK_ROW = `${testId("port-row")}[data-kind="input"][data-port="task"]`;
export const START_INSPECTOR = testId("start-inspector");
/** The Run's input, as the Start inspector prints it. */
export const START_INPUT = `${START_INSPECTOR} ${testId("start-input-text")}`;
export const EDGE_PANEL = testId("edge-detail-panel");
/** The first condition row of the edge panel — `verdict eq pass` on reviewer → End. */
export const EDGE_CONDITION = `${EDGE_PANEL} ${testId("condition-row")}`;
/**
 * The row's leading slot, not the dot in it: on row hover the resting dot is
 * `display: none` (it turns into the select ring), so a hole drawn on it
 * collapsed under the reader's own pointer, the card jumped away, the row lost
 * its hover, the dot came back — a flicker loop (#911, human test). The slot
 * keeps its box in both states.
 */
export const RUN_SELECT = testId("run-select-control");
export const CLEANUP_MODAL = testId("cleanup-confirm-modal");
export const SETTINGS_BUTTON = testId("open-settings");

/**
 * The inspector's navigation, which a read-only step still lets through (#911):
 * the Run / Edit tabs, the I/O / Orchestration tabs, the « Initial Prompt » fold,
 * and the terminal's own unfold / expand / copy. Everything else in the panel
 * that is a control stays refused while the tour shows it.
 */
export const INSPECTOR_EXPLORE = [
  testId("inspector-tab-run"),
  testId("inspector-tab-edit"),
  '[data-testid^="detail-tab-"]',
  testId("prompt-toggle"),
  TERMINAL_RESTORE,
  testId("term-expand"),
  testId("term-copy"),
];

/** The status dot, drawn three times over (#911): the tour's Run only ever shows green. */
export const STATUS_DOTS: TourIllustration = {
  kind: "rows",
  rows: [
    { label: "fix-login", detail: "waits for you", dotClass: "bg-st-await" },
    { label: "add-tests", detail: "running", dotClass: "bg-st-running", pulse: true },
    { label: OVERVIEW_RUN_NAME, detail: "finished", dotClass: "bg-st-done" },
  ],
};
export const STATS_BUTTON = testId("open-stats");

export function runRow(id: string): string {
  return `[data-run-row="${cssEscape(id)}"]`;
}

export function triggerRow(id: string): string {
  return `[data-trigger-row="${cssEscape(id)}"]`;
}

/** The tour's Run, as the list shows it, or `null` before the preparation answered. */
export function tourRun(app: TourAppState): TourRunSummary | null {
  const id = overviewRunId();
  return id ? (app.runs.find((r) => r.id === id) ?? null) : null;
}

/** The canvas is showing the tour's Run — which is what clicking its row does. */
export function runIsOpen(app: TourAppState): boolean {
  const id = overviewRunId();
  return id != null && app.activeRunId === id;
}

/** The open Run's pipeline, and only while it is the tour's Run. */
export function runNodes(app: TourAppState): EditorNodeDef[] {
  return runIsOpen(app) ? (app.pipeline?.nodes ?? []) : [];
}

/** The two working nodes, by id when the pipeline is ours, else in document
 *  order — a user who edited `tutorial-overview` keeps a working tour. */
export function worker(app: TourAppState, id: "implementer" | "reviewer"): EditorNodeDef | null {
  const nodes = runNodes(app);
  const byId = nodes.find((n) => n.id === id);
  if (byId) return byId;
  const workers = nodes.filter((n) => n.type !== "start" && n.type !== "end");
  return workers[id === "implementer" ? 0 : 1] ?? null;
}

export function marker(app: TourAppState, type: "start" | "end"): EditorNodeDef | null {
  return runNodes(app).find((n) => n.type === type) ?? null;
}

export function nodeSel(node: EditorNodeDef | null): string[] {
  return node ? [`.react-flow__node[data-id="${cssEscape(node.id)}"]`] : [];
}

export function selectedNode(app: TourAppState, node: EditorNodeDef | null): boolean {
  return node != null && app.selection.kind === "node" && app.selection.id === node.id;
}

/** reviewer → End — the edge the Run actually took. Its index is its canvas key. */
export function endEdgeIndex(app: TourAppState): number {
  const reviewer = worker(app, "reviewer");
  const end = marker(app, "end");
  if (!reviewer || !end || !app.pipeline) return -1;
  return app.pipeline.edges.findIndex((e) => e.source.node === reviewer.id && e.target.node === end.id);
}

/**
 * implementer is selected and its inspector is up — on either tab. The reader
 * may explore the Edit tab from the read-only panel (#911); a condition that
 * only knew the Run tab would drop the step back to « Click implementer » and
 * dim the very panel they were looking at.
 */
export function implementerOpen(o: TourObservation): boolean {
  return (
    selectedNode(o.app, worker(o.app, "implementer")) && (o.present(INSPECTOR_RUN) || o.present(INSPECTOR_EDIT))
  );
}

/**
 * The reader left the inspector on its Edit tab (#911): what the Run tab shows —
 * the outputs, the terminal — is not on screen, and the step points at the tab
 * instead of starving on a target that will not appear by itself.
 */
export function onEditTab(o: TourObservation): boolean {
  return !o.present(INSPECTOR_RUN) && o.present(INSPECTOR_TAB_RUN);
}

export function endEdgeOpen(o: TourObservation): boolean {
  const i = endEdgeIndex(o.app);
  return i >= 0 && o.app.selection.kind === "edge" && o.app.selection.edgeIndex === i && o.present(EDGE_PANEL);
}

export function startOpen(o: TourObservation): boolean {
  return selectedNode(o.app, marker(o.app, "start")) && o.present(START_INSPECTOR);
}

export function isArchived(app: TourAppState): boolean {
  return tourRun(app)?.status === "archived";
}

/**
 * How long a step waits for its target. The first ones land right after a fetch
 * the click itself triggered — the Run's pipeline, the node's detail — and a
 * tour that gave up while the app was still loading would blame the reader for a
 * round trip.
 */
export const READING_TIMEOUT_MS = 15_000;

/** A step that shows one thing and is acknowledged with Next. */
export function readStep(def: {
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

