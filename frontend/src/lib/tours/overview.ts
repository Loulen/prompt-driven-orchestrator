/**
 * The *Overview* tour (#911, spec #910) — the **tour préface**: a walk around the
 * screen on a real, finished Run, before *First run* teaches a single gesture in
 * depth. It needs **no harness**: the Run it visits is a pipeline of `script`
 * nodes (ADR-0017), which finish in seconds and cost nothing.
 *
 * Eighteen steps, twelve of them gestures that advance only on the observed state
 * (open the Run, click implementer, open its `code` output, close the viewer,
 * unfold the terminal, the reviewer → End edge, Start, back to Runs, hover the
 * status icon, archive, Triggers, Pipelines); the other six are read and
 * acknowledged with Next. Two
 * of the gestures ask for a click only when what they show is not on screen: the
 * folded terminal of a finished node, and the Runs tab a reader left.
 *
 * Its intro card prepares four things (`overviewPreparation.ts`): the training
 * repository, the `tutorial-overview` pipeline, an example Trigger that is
 * harmless by construction, and a completed Run. Its teardown removes the
 * Trigger at any exit; the pipeline and the Run stay the user's. The Run and the
 * Trigger carry this browser's Identité de tour (ADR-0071 §5).
 *
 * Copy rules inherited from the other tours: title is an imperative, body is two
 * sentences at most, and the other tours are pointed to in one sentence, never a
 * button.
 */

import type { TourDef, TourStep } from "../tourDef";
import { TUTORIAL_REPO_PATH } from "./firstRunPreparation";
import { CANVAS_STEPS } from "./overviewCanvasSteps";
import { TUTORIAL_OVERVIEW_PIPELINE_ID, exampleTriggerName, overviewRunName } from "./overviewPipeline";
import {
  exampleTriggerId,
  overviewRunId,
  prepareCompletedRun,
  prepareExampleTrigger,
  preparePipelineStep,
  prepareRepoStep,
  removeExampleTrigger,
} from "./overviewPreparation";
import {
  CLEANUP_MODAL,
  NEW_PIPELINE_BUTTON,
  NEW_RUN_BUTTON,
  PIPELINES_TAB,
  PIPELINE_ROW,
  READING_TIMEOUT_MS,
  RUNS_TAB,
  RUNS_TAB_OPEN,
  SETTINGS_BUTTON,
  STATS_BUTTON,
  STATUS_ICONS,
  TRIGGERS_LIST,
  TRIGGERS_TAB,
  cleanupItem,
  hovered,
  isArchived,
  readStep,
  runRow,
  runSelect,
  triggerRow,
} from "./overviewReading";
import { brandName } from "../../shared/ui/brand/brand";

/** The second half: the left panel's tabs, the Run's status and archive, then the top bar. */
const APP_STEPS: TourStep[] = [
  {
    id: "runs-tab",
    title: "The Runs tab",
    // The three steps after this one read the Runs list: a reader who left it
    // for Triggers or Pipelines is sent back first (FP #911).
    body: (o) =>
      o.present(RUNS_TAB_OPEN)
        ? "Sort and filter your Runs here, and start a new one with New Run. The First run tour fills that form with you."
        : "Click Runs. Your Runs are sorted and filtered there, and New Run starts one — the First run tour fills that form with you.",
    target: (o) => {
      if (!o.present(RUNS_TAB_OPEN)) return [RUNS_TAB];
      return o.present(NEW_RUN_BUTTON) ? [RUNS_TAB, NEW_RUN_BUTTON] : [RUNS_TAB];
    },
    waitingFor: "the Runs tab",
    targetTimeoutMs: READING_TIMEOUT_MS,
    advanceHint: "advances when the tab opens",
    confirm: (o) => o.present(RUNS_TAB_OPEN),
    done: (o) => o.present(RUNS_TAB_OPEN),
  },
  {
    id: "status-icon",
    title: "The status icon",
    body: "Its shape tells you where the Run is: a check means finished, a turning circle means running, a question mark means it waits for you. Hover it for details.",
    target: () => {
      const id = overviewRunId();
      return id ? [runSelect(id)] : [];
    },
    illustration: STATUS_ICONS,
    waitingFor: "the status icon of your Run",
    failureHint: "It is on your Run's row, in the Runs tab.",
    targetTimeoutMs: READING_TIMEOUT_MS,
    skippable: true,
    // The gesture the body asks for, named in the footer the way the other
    // gestures are. Hover is the whole of it: there is nothing to click, and the
    // details it shows are the row's own tooltip. Only this browser's Run counts
    // (ADR-0071 §5): another player's row is not « your Run ».
    advanceHint: "Hover the status icon of your Run",
    done: (o) => {
      const id = overviewRunId();
      return id != null && o.present(hovered(runSelect(id)));
    },
  },
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
        return "Archived: the worktree is gone and the row shows the archive icon. The outputs are still there to read.";
      }
      if (o.present(CLEANUP_MODAL)) {
        return "Cleanup removes the Run's worktree and keeps its history and outputs. Skip leaves the Run as it is.";
      }
      const id = overviewRunId();
      return id && o.present(cleanupItem(id))
        ? "Choose Cleanup, then confirm."
        : "Open the ⋯ menu of your Run's row, choose Cleanup, then confirm. The worktree goes; the Run and its outputs stay.";
    },
    // Three beats, one card: the row, the menu its ⋯ opens (a portal, lit as a
    // zone so it is not under the blocker), then the confirmation.
    target: (o) => {
      if (o.present(CLEANUP_MODAL)) return [CLEANUP_MODAL];
      const id = overviewRunId();
      if (!id) return [];
      // Only this Run's menu: another row's Cleanup would archive the wrong Run.
      return o.present(cleanupItem(id)) ? [cleanupItem(id)] : [runRow(id)];
    },
    soft: (o) => {
      const id = overviewRunId();
      return !o.present(CLEANUP_MODAL) && id != null && o.present(cleanupItem(id));
    },
    waitingFor: "your Run in the list",
    failureHint: "An archived Run moves to the Archived section at the bottom of the list.",
    targetTimeoutMs: READING_TIMEOUT_MS,
    skippable: true,
    advanceHint: "advances once the Run is archived",
    // Once archived the card stops for a Next: the archive icon is the thing to see.
    confirm: (o) => isArchived(o.app),
    done: (o) => isArchived(o.app),
  },
  {
    id: "triggers-tab",
    title: "Open the Triggers tab",
    body: (o) =>
      o.present(TRIGGERS_LIST)
        ? `This is \`${exampleTriggerName()}\`, set up for the tour: every 15 minutes it would launch ${TUTORIAL_OVERVIEW_PIPELINE_ID}, but its guard always says no. A Trigger launches a pipeline on a schedule, when its guard agrees.`
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
  blurb: "Canvas, panels, nodes, edges, Triggers. No agent needed.",
  minutes: 4,
  steps: [...CANVAS_STEPS, ...APP_STEPS],
  intro: {
    title: `Tour of ${brandName()}`,
    body: "A walk around the screen on a real Run: the canvas, the panels, a node and its output, an edge, the Runs, Triggers and Pipelines tabs. The tour prepares everything first; nothing needs an agent.",
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
      // Getters: the names carry the Identité de tour, read when the card shows
      // them rather than when this module loads.
      {
        id: "trigger",
        get pending() {
          return `Example Trigger ${exampleTriggerName()}…`;
        },
        get ready() {
          return `Example Trigger ${exampleTriggerName()} active (it never fires a Run)`;
        },
        failureTitle: "The example Trigger could not be created",
        run: prepareExampleTrigger,
      },
      {
        id: "run",
        get pending() {
          return `Run ${overviewRunName()} running…`;
        },
        get ready() {
          return `Run ${overviewRunName()} completed`;
        },
        failureTitle: "The tour's Run did not complete",
        run: prepareCompletedRun,
      },
    ],
  },
  teardown: [
    {
      id: "example-trigger",
      get failureTitle() {
        return `The example Trigger ${exampleTriggerName()} could not be removed`;
      },
      run: removeExampleTrigger,
    },
  ],
  recapIntro: `You toured ${brandName()} on a real Run of ${TUTORIAL_OVERVIEW_PIPELINE_ID}:`,
  recap: (app) => [
    { label: "Canvas", text: "nodes linked by edges, listed and detailed around it" },
    { label: "Edges", text: "conditions on outputs, like `verdict = pass`" },
    { label: "Run", text: isArchived(app) ? "archived, its outputs stay" : "still in your list, with its worktree" },
    { label: "Trigger", text: "the example one is gone" },
  ],
  outro: {
    closing: "The pipeline and the Run stay yours.",
  },
};
