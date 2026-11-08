/**
 * The *First pipeline* tour (#823, spec #821) — pure data.
 *
 * Order and content come from the ticket: create a pipeline, build an
 * **implementer** and a **tester**, give the tester a `verdict` enum and an
 * annotated screenshot, then wire the loop (`verdict eq fail` back to the
 * implementer, `verdict eq pass` to End), save, validate it for the projects
 * that should see it, and decide whether to keep it.
 *
 * Copy rules the implementer of a new step must keep (design round 1):
 * - Title is an imperative, body is two sentences at most.
 * - Every free input carries the exact block to paste.
 * - **"Interactive" never "opens a terminal".** Every node's terminal is open;
 *   Interactive means the node cannot complete until a human releases it, and that
 *   it will wait on your answers along the way.
 */

import type { TourDef, TourStep } from "../tourDef";
import {
  ADD_MENU_NODE,
  NAME_INPUT,
  NEW_PIPELINE_DIALOG,
  NEW_PIPELINE_ERROR,
  NEW_PIPELINE_NAME,
  PROMPT_INPUT,
  agent,
  agents,
  filled,
  nodeSel,
  outputSlot,
  pipelineCreated,
  selected,
  tutorialPipelineName,
} from "./firstPipelineReading";
import { TESTER_STEPS } from "./firstPipelineTesterSteps";

export { TUTORIAL_PIPELINE_ID, tutorialPipelineName } from "./firstPipelineReading";

const IMPLEMENTER_PROMPT = `Implement what the user asked for, then run the project's tests.
Write the test results and a mermaid diagram of your change into your output file.`;

const IMPLEMENTER_OUT = "Test results and a mermaid diagram of the change";

// ---- the steps ------------------------------------------------------------

const STEPS: TourStep[] = [
  {
    id: "pipelines-tab",
    title: "Open the Pipelines tab",
    body: "Runs are on the left; pipelines live in the next tab. A pipeline is the graph a Run executes.",
    target: () => ['[data-testid="left-tab-library"]'],
    waitingFor: "the Pipelines tab",
    done: (o) => o.present('[data-testid="new-pipeline-button"]'),
  },
  {
    id: "new-pipeline",
    title: "Create a new pipeline",
    body: "Click the + at the top of the Pipelines list. A pipeline is a graph of nodes; you are about to build one with two agents.",
    target: () => ['[data-testid="new-pipeline-button"]'],
    waitingFor: "the new-pipeline button",
    done: (o) => o.present(NEW_PIPELINE_DIALOG),
  },
  {
    id: "name-pipeline",
    title: "Name the pipeline",
    body: "Paste the name below into the field. The name becomes the YAML file in your Library; the four characters at the end are this browser's, so it never collides with someone else's.",
    // Strictly the field (#837). Lighting the whole dialog put an enabled
    // `Create` inside the hole, five centimetres from `Next`, and a reader who
    // clicked it — the natural gesture — closed the dialog under a step whose
    // condition then went false: five seconds later the tour stopped on
    // « nothing was created » while the pipeline sat open in the editor.
    // Under the blocker, Create is absorbed until the step that asks for it.
    target: () => [NEW_PIPELINE_NAME],
    waitingFor: "the Name field of the New Pipeline dialog",
    failureHint: "The dialog was closed before the name was entered.",
    copyBlock: tutorialPipelineName,
    // Enter in the field creates the pipeline too, and that closes the dialog.
    // The step then has nothing left to confirm: it slides on to `Create it`,
    // which is entered satisfied and says so.
    confirm: (o) => o.present(NEW_PIPELINE_DIALOG),
    done: (o) => pipelineCreated(o) || filled(o, NEW_PIPELINE_NAME),
    // That condition holds for good once the pipeline exists, but the step's
    // whole point is a field that leaves with the dialog: `Previous` from
    // `Create it` is greyed out rather than replaying « paste the name » over a
    // screen with nothing to paste into (CONTEXT.md § « Rejeu d'étape »).
    replayNeedsTarget: true,
  },
  {
    id: "create-pipeline",
    title: "Create it",
    // Entered satisfied when Enter already did the work (see the step above),
    // or replayed with `Previous` after the click: the card then says what
    // happened rather than instructing a click that nobody can make — the
    // button is gone. It does not say HOW it was created: on a replay that was
    // the click, and « Enter already created it » would be false.
    body: (o) =>
      pipelineCreated(o)
        ? "Already created: the file is written and open in the editor, with a Start and an End marker already in place."
        : "Click Create. The file is written and opened in the editor, with a Start and an End marker already in place.",
    target: () => ['[data-testid="new-pipeline-create"]'],
    waitingFor: "the Create button",
    // A reader who kept the pipeline at the end of a previous run of this tour
    // meets a name that is already taken, and the daemon refuses. Nothing here
    // can advance, so the tour says so instead of waiting on a button that will
    // go on doing nothing (#825, FP iteration 2).
    refused: (o) => o.text(NEW_PIPELINE_ERROR),
    refusalTitle: "The pipeline could not be created",
    refusalHint: () =>
      `You may already have a ${tutorialPipelineName()} from a previous run of this tour — open it from the Pipelines list, or delete it and start the tour again.`,
    done: pipelineCreated,
  },
  {
    id: "open-add-menu",
    title: "Open the Add menu",
    body: "Click the + in the canvas toolbar. Everything you can drop on the canvas is in there.",
    target: () => ['[data-testid="toolbar-add"]'],
    waitingFor: "the canvas toolbar",
    done: (o) => o.present(ADD_MENU_NODE),
  },
  {
    id: "choose-node",
    title: "Choose Node",
    body: "Pick Node in the menu. It drops an agent node on the canvas — an agent that gets a prompt and produces files.",
    target: () => [ADD_MENU_NODE],
    waitingFor: "the Node entry of the Add menu",
    soft: true,
    done: (o) => agents(o.app).length >= 1,
    // « Pick Node in the menu » only makes sense with the menu open, and it
    // closes on the pick: replayed without it, the card would point at nothing.
    replayNeedsTarget: true,
  },
  {
    id: "select-implementer",
    title: "Select the node",
    body: "Click the new card. The right pane becomes its inspector — everything about a node is edited there.",
    target: (o) => nodeSel(agent(o.app, 0)),
    waitingFor: "the new node on the canvas",
    done: (o) => selected(o.app, agent(o.app, 0)),
  },
  {
    id: "name-implementer",
    title: "Name it implementer",
    body: "It is already called implementer, which is the name this tour uses. This field is where you would change it.",
    target: () => [NAME_INPUT],
    waitingFor: "the Name field of the selected node",
    failureHint: "This usually means the node was deselected or the pane was closed.",
    copyBlock: "implementer",
    confirm: true,
    done: (o) => (o.value(NAME_INPUT) ?? "").trim() === "implementer",
  },
  {
    id: "implementer-interactive",
    title: "Make it interactive",
    body: "Turn on Interactive. The node then cannot complete until you release it, and it will wait on your answers along the way. Its terminal is open either way.",
    target: () => ['[data-testid="node-interactive-toggle"]'],
    waitingFor: "the Interactive toggle",
    done: (o) => agent(o.app, 0)?.interactive === true,
  },
  {
    id: "implementer-worktree",
    title: "Put it in the Run worktree",
    body: "Pick Run worktree. This node then works in the branch the Run itself owns, so what it writes is what the Run carries.",
    target: () => ['[data-testid="workspace-shared"]'],
    waitingFor: "the Workspace choice",
    done: (o) => agent(o.app, 0)?.isolated_worktree === false,
  },
  {
    id: "implementer-prompt",
    title: "Give it its prompt",
    body: "Paste the block below into Prompt. This is the whole instruction the agent wakes up with.",
    target: () => [PROMPT_INPUT],
    waitingFor: "the Prompt field of the selected node",
    failureHint: "This usually means the node was deselected or the pane was closed.",
    copyBlock: IMPLEMENTER_PROMPT,
    confirm: true,
    done: (o) => filled(o, PROMPT_INPUT),
  },
  {
    id: "implementer-out",
    title: "Say what out must contain",
    body: "Open Expected content on the out port and paste the block below. It guides the agent; nothing verifies it.",
    target: () => [outputSlot(0)],
    waitingFor: "the out output of the implementer",
    copyBlock: IMPLEMENTER_OUT,
    confirm: true,
    done: (o) => !!agent(o.app, 0)?.outputs[0]?.instructions?.trim(),
  },
  ...TESTER_STEPS,
];

export const FIRST_PIPELINE_TOUR: TourDef = {
  id: "first-pipeline",
  title: "First pipeline",
  blurb: "An implementer and a tester in a loop, with a verdict.",
  minutes: 8,
  steps: STEPS,
  recapIntro: () =>
    `You built ${tutorialPipelineName()} and saved it to your Library. Here is what it does:`,
  recap: [
    {
      label: "implementer",
      text: "agent in the Run worktree, implements the request and runs the tests; interactive, so it completes only on your go.",
    },
    {
      label: "tester",
      text: "agent in an isolated worktree, also interactive, returns a verdict (pass | fail) and an annotated screenshot.",
    },
    {
      label: "Loop",
      text: "tester → implementer when verdict eq fail; tester → End when verdict eq pass.",
    },
    {
      label: "Validate",
      text: "New Run offers it to the projects you validated it for; until then it is a test pipeline, behind Show test pipelines.",
    },
  ],
  // #825 — what the card of a Full tour's *previous* leg says when that leg was
  // refused. *First run* is the one that can be stopped by the machine (no
  // harness on PATH, no sandbox); this tour never spawns anything, so a refusal
  // there is no reason to abandon the chain.
  chainNote: "This one does not need a running agent: it only builds a pipeline.",
};
