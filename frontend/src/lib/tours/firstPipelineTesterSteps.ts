/**
 * The second half of the *First pipeline* tour (#823): the tester node, its
 * verdict and screenshot outputs, the edge from Start (CORE-241), the loop that
 * wires the tester back to the implementer and on to End, then save, validate
 * and keep-or-delete. Pure data, spliced after the
 * first half in `firstPipeline.ts`; the copy rules there apply here too.
 */

import type { TourObservation, TourStep } from "../tourDef";
import {
  ADD_MENU_NODE,
  DELETE_CONFIRM,
  SAVE_DIALOG,
  NAME_INPUT,
  OUT_PORT,
  PROMPT_INPUT,
  VALIDATED_FOR_ALL,
  VALIDATED_FOR_PROJECTS,
  VALIDATE_BUTTON,
  VALIDATE_MODAL,
  agent,
  agents,
  builtPipelineId,
  carryOutEdgeStep,
  conditionStep,
  cssEscape,
  edgeIndex,
  edgeSel,
  edgeSelected,
  endNode,
  filled,
  nodeSel,
  outputSlot,
  selected,
  startNode,
} from "./firstPipelineReading";

const TESTER_PROMPT = `Review the implementer's change and exercise the feature yourself.
Put verdict: pass in your output's frontmatter when it holds up, verdict: fail otherwise, and attach an annotated screenshot.`;

const TESTER_OUT = "A short verdict, and what failed if anything did";
const TESTER_SHOT = "Annotated screenshot of the feature";

/** The pipeline is saved, and the Library lists it. */
function saved(o: TourObservation): boolean {
  return !o.app.dirty && !!o.app.pipelineId && o.app.libraryPipelineIds.includes(o.app.pipelineId);
}

/** The Validate modal is closed, and the tab bar shows the pipeline validated. */
function validated(o: TourObservation): boolean {
  return !o.present(VALIDATE_MODAL) && (o.present(VALIDATED_FOR_ALL) || o.present(VALIDATED_FOR_PROJECTS));
}

export const TESTER_STEPS: TourStep[] = [
  {
    id: "open-add-menu-2",
    title: "Add the second node",
    body: "Open the + menu again. The tester is a second agent, not a second job for the first one.",
    target: () => ['[data-testid="toolbar-add"]'],
    waitingFor: "the canvas toolbar",
    done: (o) => o.present(ADD_MENU_NODE),
  },
  {
    id: "choose-node-2",
    title: "Choose Node again",
    body: "Pick Node. A second agent card lands on the canvas.",
    target: () => [ADD_MENU_NODE],
    waitingFor: "the Node entry of the Add menu",
    soft: true,
    done: (o) => agents(o.app).length >= 2,
    // Same as the first « Choose Node »: the menu closes on the pick.
    replayNeedsTarget: true,
  },
  {
    id: "select-tester",
    title: "Select the second node",
    body: "Click the new card so the inspector follows it. It reads implementer-2 until you name it.",
    target: (o) => nodeSel(agent(o.app, 1)),
    waitingFor: "the second node on the canvas",
    done: (o) => selected(o.app, agent(o.app, 1)),
  },
  {
    id: "name-tester",
    title: "Name it tester",
    body: "Replace the name with the block below, then press Next. A node's name is how edges and conditions refer to it.",
    target: () => [NAME_INPUT],
    waitingFor: "the Name field of the selected node",
    failureHint: "This usually means the node was deselected or the pane was closed.",
    copyBlock: "tester",
    confirm: true,
    done: (o) => (o.value(NAME_INPUT) ?? "").trim() === "tester",
  },
  {
    id: "tester-interactive",
    title: "Make the tester interactive too",
    body: "Turn on Interactive. It cannot complete until you release it, and it will wait on your answers along the way.",
    target: () => ['[data-testid="node-interactive-toggle"]'],
    waitingFor: "the Interactive toggle",
    done: (o) => agent(o.app, 1)?.interactive === true,
  },
  {
    id: "tester-worktree",
    title: "Leave it in an isolated worktree",
    body: "An isolated worktree gives this node its own copy of the repo, so the tester reads the change instead of sitting in the middle of it. That is already the default for a new agent — compare it with the implementer you just moved.",
    target: () => ['[data-testid="workspace-isolated"]'],
    waitingFor: "the Workspace choice",
    done: (o) => agent(o.app, 1)?.isolated_worktree === true,
  },
  {
    id: "tester-prompt",
    title: "Give the tester its prompt",
    body: "Paste the block below into Prompt. It asks for a verdict and a screenshot — the two things the rest of the graph reads.",
    target: () => [PROMPT_INPUT],
    waitingFor: "the Prompt field of the selected node",
    failureHint: "This usually means the node was deselected or the pane was closed.",
    copyBlock: TESTER_PROMPT,
    confirm: true,
    done: (o) => filled(o, PROMPT_INPUT),
  },
  {
    id: "tester-out",
    title: "Say what the tester's out must contain",
    body: "Open Expected content on out and paste the block below. Same contract as the implementer's, different job.",
    target: () => [outputSlot(0)],
    waitingFor: "the out output of the tester",
    copyBlock: TESTER_OUT,
    confirm: true,
    done: (o) => !!agent(o.app, 1)?.outputs[0]?.instructions?.trim(),
  },
  {
    id: "tester-verdict",
    title: "Declare the verdict field",
    body: "Add a field named verdict, set its type to enum, and allow pass and fail. Edges read this frontmatter to decide where the Run goes next.",
    // Two beats (FP of CORE-227): « + field » alone while out declares no
    // field (the whole editor lit a 443 px zone for a 52 px button), then the
    // editor, whose name, type and allowed values the step asks to fill in.
    target: (o) =>
      Object.keys(agent(o.app, 1)?.outputs[0]?.frontmatter ?? {}).length === 0
        ? [`${outputSlot(0)} [data-testid="add-schema-field"]`]
        : [`${outputSlot(0)} [data-testid="output-schema-editor"]`],
    waitingFor: "the output schema editor",
    done: (o) => {
      const decl = agent(o.app, 1)?.outputs[0]?.frontmatter?.verdict;
      const allowed = decl?.allowed ?? [];
      return decl?.type === "enum" && allowed.includes("pass") && allowed.includes("fail");
    },
  },
  {
    id: "add-image-port",
    title: "Add a second output",
    body: "Click + Add on Outputs. One port per kind of thing a node produces — the screenshot is not the verdict.",
    target: () => ['[data-testid="add-output-port"]'],
    waitingFor: "the Outputs section of the inspector",
    done: (o) => (agent(o.app, 1)?.outputs.length ?? 0) >= 2,
  },
  {
    id: "name-image-port",
    title: "Name it image_list and set its type",
    body: "Rename the new port to image_list and pick Image List as its Type. A typed port is what lets the UI show the screenshots instead of a path.",
    target: () => [outputSlot(1)],
    waitingFor: "the second output of the tester",
    copyBlock: "image_list",
    done: (o) => {
      const port = agent(o.app, 1)?.outputs[1];
      return port?.name === "image_list" && port?.port_type === "image_list";
    },
  },
  {
    id: "image-port-expected",
    title: "Say what the screenshot must show",
    body: "Open Expected content on image_list and paste the block below. Without it the agent guesses what to capture.",
    target: () => [outputSlot(1)],
    waitingFor: "the second output of the tester",
    copyBlock: TESTER_SHOT,
    confirm: true,
    done: (o) => !!agent(o.app, 1)?.outputs[1]?.instructions?.trim(),
  },
  {
    // CORE-241: the daemon's scaffold puts Start and End on the canvas with no
    // edge, and a Run starts only the nodes Start is wired to. Without this
    // step the tour ended on a pipeline whose Runs never started anything.
    id: "edge-start-implementer",
    title: "Connect Start to implementer",
    // #846 — there is no output dot any more (#844, ADR-0071): the whole border
    // of a card is the drag-source. Both cards are the target, rim included, so
    // the hole lights the very strip the press has to land on.
    body: "Press anywhere on the border of the Start marker (it glows amber) and drag onto implementer. A Run hands its prompt to the nodes Start is wired to and starts them: without this edge, it would start nothing.",
    target: (o) => [...nodeSel(startNode(o.app)), ...nodeSel(agent(o.app, 0))],
    waitingFor: "the Start marker and the implementer card",
    done: (o) => edgeIndex(o.app.pipeline, startNode(o.app), agent(o.app, 0)) >= 0,
  },
  {
    id: "edge-implementer-tester",
    title: "Connect implementer to tester",
    body: "Now press on the border of the implementer's card and drag onto tester. The edge is the hand-off: when the first finishes, the second starts.",
    target: (o) => [...nodeSel(agent(o.app, 0)), ...nodeSel(agent(o.app, 1))],
    waitingFor: "the two agent cards",
    done: (o) => edgeIndex(o.app.pipeline, agent(o.app, 0), agent(o.app, 1)) >= 0,
  },
  carryOutEdgeStep({
    id: "edge-tester-implementer",
    title: "Draw the way back",
    to: (app) => agent(app, 0),
    body: "Now press on the border of the tester's card and drag onto implementer. That second edge is what makes this a loop rather than a line.",
    // The tester grew a second output a few steps ago. A new edge carries the
    // FIRST declared one (ADR-0073), which is `out`; an edge carrying only
    // `image_list` would leave the next condition no verdict to read (#825).
    note: "A new edge carries the tester's first output, out — not image_list, the screenshot, which has no verdict to route on.",
    waitingFor: "the two agent cards",
  }),
  {
    id: "select-loop-edge",
    title: "Select the edge back to implementer",
    body: (o) =>
      edgeSelected(o.app, agent(o.app, 0))
        ? "The edge is already selected. Its inspector on the right is where a condition is authored."
        : "Click the edge you just drew. Its inspector opens on the right, where a condition is authored.",
    target: (o) => edgeSel(o.app, agent(o.app, 1), agent(o.app, 0), OUT_PORT),
    waitingFor: "the tester → implementer edge",
    done: (o) => edgeSelected(o.app, agent(o.app, 0)),
  },
  conditionStep({
    id: "loop-condition",
    title: "Only loop back on a failure",
    to: (app) => agent(app, 0),
    value: "fail",
    why: "The edge now fires only when the tester's frontmatter says the change did not hold up.",
  }),
  carryOutEdgeStep({
    id: "edge-tester-end",
    title: "Connect tester to End",
    to: endNode,
    body: "Press on the border of the tester's card again and drag onto the End marker. End is where a Run stops, and it needs a route in.",
    // Same two outputs, same consequence: the `verdict = pass` of the next
    // step can only be written on an edge that carries the verdict (#825).
    note: "Again the edge carries out, the first output: the condition below it reads the verdict in out's frontmatter.",
    waitingFor: "the tester card and the End marker",
  }),
  {
    id: "select-end-edge",
    title: "Select the edge to End",
    body: (o) =>
      edgeSelected(o.app, endNode(o.app))
        ? "The edge is already selected. Same inspector, one more condition to write."
        : "Click that last edge. Same inspector, one more condition to write.",
    target: (o) => edgeSel(o.app, agent(o.app, 1), endNode(o.app), OUT_PORT),
    waitingFor: "the tester → End edge",
    done: (o) => edgeSelected(o.app, endNode(o.app)),
  },
  conditionStep({
    id: "end-condition",
    title: "Finish only on a pass",
    to: endNode,
    value: "pass",
    why: "Both routes out of the tester are now explicit: pass ends the Run, fail sends it round again.",
  }),
  {
    id: "save",
    title: "Save the pipeline",
    // The step slides on once saved, so the first body is only seen on a replay
    // with `Previous` (CONTEXT.md § « Rejeu d'étape »): it says what was done
    // rather than asking for a click that is already made.
    body: (o) =>
      saved(o)
        ? "Saved: the pipeline is in your Library as a version. Until you save, your changes are a draft."
        : o.present(SAVE_DIALOG)
          ? "Click Save in the window. Each save numbers a new version of the pipeline; the earlier ones stay under Versions."
          : "Click Save. Until you do, all of this is a draft: Save makes it a version in your Library.",
    // Two beats, as Validate: the button, then the dialog it opens, lit rather
    // than drawn under the dimming (CORE-227, CORE-232).
    target: (o) => (o.present(SAVE_DIALOG) ? [SAVE_DIALOG] : ['[data-testid="save-button"]']),
    soft: (o) => o.present(SAVE_DIALOG),
    waitingFor: "the Save button",
    // Not skippable: everything after this reads the saved file.
    done: saved,
  },
  {
    // A pipeline PDO creates is born a test pipeline: New Run offers it only
    // behind « Show test pipelines ». Validating it is what makes the one just
    // built show up at launch.
    id: "validate",
    title: "Validate the pipeline",
    // Two beats, one card: the button, then the modal it opens.
    body: (o) =>
      validated(o)
        ? "Validated: New Run offers it to the projects you chose. Validate, next to Save, changes them."
        : o.present(VALIDATE_MODAL)
        ? "Tick All projects, or the projects that should see it, then Save validation. Leaving everything unticked keeps it a test pipeline."
        : "Click Validate, next to Save. A new pipeline is a test pipeline: New Run hides it until you validate it for a project, or for all of them.",
    target: (o) => (o.present(VALIDATE_MODAL) ? [VALIDATE_MODAL] : [VALIDATE_BUTTON]),
    soft: (o) => o.present(VALIDATE_MODAL),
    waitingFor: "the Validate button, next to Save",
    failureHint: "It appears in the tab bar once the pipeline is saved.",
    // A reader who keeps a test pipeline skips; the recap says what that means.
    skippable: true,
    advanceHint: "advances once validated",
    done: validated,
  },
  {
    id: "keep-or-delete",
    title: "Keep it, or delete it",
    // Two beats, one card: the trash, then the confirmation it opens.
    body: (o) =>
      o.present(DELETE_CONFIRM)
        ? "Delete removes the YAML and its prompt files from disk, for good. Cancel leaves the row alone, and Skip ends the tour without deleting anything."
        : "The row is yours now: open it again later, or use its trash to remove it. Skip keeps it.",
    // Re-aims onto the confirmation, the way `open-output` re-aims onto the
    // artifact. Without it the card instructs a click it then prevents: the
    // dialog is drawn centred, outside the hole, and both of its buttons come
    // back as the projecteur's blocker (FP finding, #825).
    target: (o) => {
      if (o.present(DELETE_CONFIRM)) return [DELETE_CONFIRM];
      const id = builtPipelineId(o);
      return id ? [`[data-testid="library-row-${cssEscape(id)}"]`] : [];
    },
    waitingFor: "the pipeline's row in the Library",
    skippable: true,
    // Gone from the Library. Not read off the active tab: deleting the pipeline
    // closes its tab in the same store update, so a tour whose pipeline was the
    // only open tab saw `pipelineId` go null and waited on a row that was
    // already gone.
    done: (o) => builtPipelineId(o) === null,
  },
];
