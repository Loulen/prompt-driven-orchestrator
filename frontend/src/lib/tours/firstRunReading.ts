/**
 * How the *First run* tour (#824, #825) reads the app: the selectors of the New
 * Run form and of the Run inspector, and the questions its steps ask about the
 * Run it launched — is it open, which node is followed, how did that node end.
 * Pure: every function takes the observed state and returns an answer.
 */

import { isNodeFinished } from "../tourDef";
import type { TourAppState, TourObservation, TourRunNode } from "../tourDef";
import {
  TUTORIAL_OUT_PORT,
  TUTORIAL_REPO_NAME,
  TUTORIAL_RUN_PIPELINE_ID,
} from "./firstRunPreparation";

// ---- selectors ------------------------------------------------------------

const testId = (id: string) => `[data-testid="${id}"]`;

export const RUNS_TAB = testId("left-tab-runs");
export const NEW_RUN_BUTTON = testId("new-run-button");
export const NAME_INPUT = testId("run-name-input");
export const AUTO_NAME = testId("auto-name-checkbox");
export const REPO_INPUT = testId("target-repo-input");
export const BROWSE_TRIGGER = testId("repo-browse-trigger");
export const EXPLORER = testId("repo-browser-modal");
export const REPO_VALID = testId("repo-valid");
export const PIPELINE_TRIGGER = testId("pipeline-select");
export const PIPELINE_OPTION = testId(`pipeline-select-option-${TUTORIAL_RUN_PIPELINE_ID}`);
/** The trigger, *and* it is holding the tutorial pipeline — one selector, so the
 *  step's condition never has to parse a display name out of a label. */
export const PIPELINE_CHOSEN = `${PIPELINE_TRIGGER}[data-pipeline-id="${TUTORIAL_RUN_PIPELINE_ID}"]`;
export const AGENT_TRIGGER = testId("run-agent-control");
export const AGENT_POPOVER = testId("run-agent-control-popover");
/** The reserved floor profile's id is fixed forever; its NAME is renameable. */
export const AGENT_DEFAULT_ROW = testId("run-agent-control-choice-default");
export const AGENT_DEFAULT_CHOSEN = `${AGENT_TRIGGER}[data-choice="profile:default"]`;
export const SKILLS_TRIGGER = testId("run-skill-selector");
export const SKILLS_POPOVER = testId("run-skill-selector-popover");
export const SKILLS_PDO_FOLDER = testId("run-skill-selector-folder-skf-pdo");
export const ORCHESTRATE_ID = "pdo-orchestrate";
export const INTERACTIVE_ID = "pdo-interactive";
export const skillRow = (id: string) => testId(`run-skill-selector-option-${id}`);
/** The skill's **ticked box** inside the popover (#849). The list of active skills
 *  under the trigger is gone — the popover is where a selection is read now — and
 *  since a pick no longer folds the picker away, the box the user just ticked is
 *  still on screen for the checklist to read. Inherited skills are ticked too
 *  (disabled), which is what makes an instance that already delivers both pass. */
const skillChecked = (id: string) => `${testId(`run-skill-selector-check-${id}`)}:checked`;
export const PROMPT_INPUT = testId("input-textarea");
export const LAUNCH_BUTTON = testId("launch-button");
export const LAUNCH_ERROR = testId("launch-error");

/** Every explorer row shares one testid; `data-entry-name` is what names this one. */
export const TUTORIAL_ENTRY = `${testId("repo-browse-entry")}[data-entry-name="${TUTORIAL_REPO_NAME}"]`;

// ---- the reading half (#825) ----------------------------------------------

/** The Run inspector — the pane the node's terminal, guard and outputs live in. */
export const INSPECTOR_RUN = testId("inspector-pane-run");
export const TERMINAL = testId("tmux-terminal");
/** The chevron of Approve ▾: its menu holds « Auto-approve when ready ». */
export const RELEASE_BUTTON = testId("approve-btn-menu");
export const OUT_ROW = `${testId("port-row")}[data-kind="output"][data-port="${TUTORIAL_OUT_PORT}"]`;
export const OUT_MODAL = `${testId("artifact-modal")}[data-port="${TUTORIAL_OUT_PORT}"]`;

/** Attribute selectors carry ids the daemon sanitised, but a quote in one would
 *  break the selector silently rather than fail loudly. */
function cssEscape(value: string): string {
  return value.replace(/["\\]/g, "\\$&");
}

/** The row of the Run this tour launched, in the left panel. */
export function runRowSel(app: TourAppState): string | null {
  const id = app.latestRun?.id;
  return id ? `[data-run-row="${cssEscape(id)}"]` : null;
}

/**
 * The canvas is showing the Run this tour launched (#825). A question rather
 * than a tab-id comparison inlined twice: the step that waits on it also has to
 * *say* whether it already happened.
 */
export function runIsOpen(app: TourAppState): boolean {
  return app.latestRun != null && app.activeRunId === app.latestRun.id;
}

/**
 * The one agent node of the training pipeline — the node every reading step is
 * about. Resolved from the Run rather than hard-coded on `assistant`: a user who
 * edited `tutorial-interactive` (the preparation leaves their edit alone, by
 * design) keeps a working tour.
 *
 * `null` until the Run is open: its nodes come from the detail the inspector
 * loads, which is exactly what step 10 asks the user to trigger.
 */
export function followedNode(app: TourAppState): TourRunNode | null {
  return app.latestRun?.nodes.find((n) => n.type === "agent") ?? null;
}

/** xyflow stamps `data-id` with the node id — the one canvas target that is stable. */
export function nodeSel(node: TourRunNode | null): string[] {
  return node ? [`.react-flow__node[data-id="${cssEscape(node.id)}"]`] : [];
}

/** The node stopped, but not on `completed` — failed, stopped, skipped. Said in
 *  one word, because three steps of the tour change their mind about it. */
export function endedBadly(node: TourRunNode | null): boolean {
  return isNodeFinished(node) && node?.status !== "completed";
}

/**
 * How the recap names where the node got to. A tour that was skipped early has
 * no business claiming the node finished.
 *
 * It names the **status**, and nothing about how the node got there: « finished
 * on its own » was false for every reader whose agent stopped to ask a question
 * — `completed` covers the node that ended by itself and the one that ended
 * because somebody answered it in the terminal (FP finding, #825). That is the
 * same claim the wait card stopped making one step earlier; the summary the
 * reader takes away must not put it back.
 */
export function nodeEnding(node: TourRunNode | null): string {
  if (node == null) return "not opened";
  if (node.status === "completed") return "finished `completed`";
  if (isNodeFinished(node)) return `ended \`${node.status}\``;
  return "still running";
}

/** The inspector is showing THIS node: the Run pane is up, and the canvas
 *  selection is on it. Both, because the pane survives a change of selection. */
export function inspectorShows(o: TourObservation, node: TourRunNode | null): boolean {
  if (!node) return false;
  return (
    o.present(INSPECTOR_RUN) &&
    o.app.selection.kind === "node" &&
    o.app.selection.id === node.id
  );
}

export function filled(o: TourObservation, selector: string): boolean {
  return (o.value(selector) ?? "").trim().length > 0;
}

/** Is this skill ticked in the picker — by the user, or by a coarser tier? */
export function hasSkill(o: TourObservation, id: string): boolean {
  return o.present(skillChecked(id));
}

/**
 * How long a reading step waits for its target. Longer than the form's five
 * seconds on purpose: each of these lands right after a fetch the click itself
 * triggered — the Run's detail, the pipeline of the tab that just opened — and a
 * tour that gave up while the app was still loading what it asked for would be
 * blaming the reader for a round trip.
 */
export const READING_TIMEOUT_MS = 15_000;
