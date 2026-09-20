/**
 * The *First run* tour (#824, spec #821) — pure data, plus the two idempotent
 * preparations its first step depends on.
 *
 * It fills the New Run form, field by field, with the user doing every gesture,
 * and ends on Launch. Nine steps, and each one is an idea rather than a keystroke:
 * the menu-opening beats are folded into the step that chooses in them (the target
 * re-aims from the trigger to the option), because "now open this menu" is not
 * something anyone needs a card for.
 *
 * Copy rules inherited from *First pipeline* (design round 1):
 * - Title is an imperative, body is two sentences at most.
 * - Every free input carries the exact block to paste.
 * - Name what is written on the button. The ticket says "Start"; the button says
 *   **Launch**, so the tour says Launch (design Q1).
 *
 * The two preparations go through verbs that know nothing about tours (ADR-0071
 * §3): a generic repo-creation verb, and the ordinary pipeline API. Both are
 * reused when what they make is already there, so replaying the tour is free and
 * a user who edited the training repo keeps their edit.
 */

import { ApiError, createPipeline, createRepo, fetchPipelines, savePipeline } from "../../api";
import {
  isNodeFinished,
  type TourAppState,
  type TourDef,
  type TourObservation,
  type TourRunNode,
  type TourStep,
} from "../tour";

/** Where the training repository is created, and the folder the explorer opens on. */
export const TUTORIAL_REPO_PARENT = "/tmp";
export const TUTORIAL_REPO_NAME = "pdo-tutorial";
export const TUTORIAL_REPO_PATH = `${TUTORIAL_REPO_PARENT}/${TUTORIAL_REPO_NAME}`;

/** The one-node pipeline the tour launches. A normal Library pipeline: no prefix,
 *  no tag — the name already says what it is (design Q8). */
export const TUTORIAL_RUN_PIPELINE_ID = "tutorial-interactive";

/** The block step 8 asks the user to paste. Deliberately concrete: the agent edits
 *  `notes.txt` and writes its output file, which the reading tour then shows. */
export const TUTORIAL_PROMPT =
  "Add a line saying hello from PDO to notes.txt, then write a short summary of what you did in your output file.";

/** The line step 12 asks the user to type INTO the agent's own session (#825).
 *  An interactive node waits for a human; this is what telling it "go ahead"
 *  looks like, in the agent's language rather than the app's. */
export const TUTORIAL_TERMINAL_LINE = "I am done, please write your summary and finish.";

/** The output port the training pipeline declares — the file step 15 opens. */
const TUTORIAL_OUT_PORT = "out";

const TUTORIAL_OUT_EXPECTED = "Summary of what you changed";

const README = `# pdo-tutorial

A throwaway repository PDO created for the First run tour. Delete it whenever you like.
`;

const NOTES = `Notes\n`;

/**
 * The training pipeline's document: one interactive node between Start and End,
 * no harness pinned (Inherit), one markdown output that says what it must contain.
 *
 * Interactive is the whole point — the node waits for the user instead of running
 * to completion on its own, which is what makes the Run worth looking at.
 *
 * All three nodes carry a `view` (#825, FP iteration 2). Only `assistant` used to,
 * so the canvas laid Start and End out itself — in a column at x = 200, straight
 * under the agent at x = 260. The very first canvas a newcomer sees had two cards
 * on top of each other. Three cards in a row, a step apart, is what the graph
 * actually is.
 */
const TUTORIAL_PIPELINE_YAML = `name: ${TUTORIAL_RUN_PIPELINE_ID}
version: "1.0"

variables: {}

nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
    view: { x: 80, y: 120 }
  - id: assistant
    name: assistant
    type: agent
    interactive: true
    isolated_worktree: false
    inputs:
      - name: task
    outputs:
      - name: out
        port_type: markdown
        instructions: ${TUTORIAL_OUT_EXPECTED}
    view: { x: 360, y: 120 }
  - id: end
    name: End
    type: end
    inputs:
      - name: result
    view: { x: 640, y: 120 }

edges:
  - source: { node: start, port: user_prompt }
    target: { node: assistant, port: task }
  - source: { node: assistant, port: out }
    target: { node: end, port: result }
`;

const TUTORIAL_NODE_PROMPT = `Do what the user asked, in the repository you are in.
Then write a short summary of what you changed into your output file.`;

// ---- preparation ----------------------------------------------------------

async function prepareRepo(): Promise<void> {
  await createRepo(TUTORIAL_REPO_PARENT, TUTORIAL_REPO_NAME, [
    { path: "README.md", content: README },
    { path: "notes.txt", content: NOTES },
  ]);
}

/**
 * Put `tutorial-interactive` in the Library, or leave the one that is already
 * there alone — **even if the user edited it** (design §1). Re-writing it would
 * quietly undo their change, and a tutorial pipeline is theirs once it exists.
 *
 * The existence check is the pipeline list rather than the create call's 409,
 * because a 409 also fires on a name that collides for another reason; the list
 * answers the question that is actually being asked.
 */
async function prepareTutorialPipeline(): Promise<void> {
  const existing = await fetchPipelines();
  if (existing.some((p) => p.id === TUTORIAL_RUN_PIPELINE_ID)) return;
  try {
    await createPipeline(TUTORIAL_RUN_PIPELINE_ID);
  } catch (e) {
    // Lost the race with another tab (or a list that was a beat stale): the
    // pipeline exists, which is all this preparation promised.
    if (!(e instanceof ApiError) || e.status !== 409) throw e;
    return;
  }
  await savePipeline(TUTORIAL_RUN_PIPELINE_ID, TUTORIAL_PIPELINE_YAML, {
    assistant: TUTORIAL_NODE_PROMPT,
  });
}

// ---- selectors ------------------------------------------------------------

const testId = (id: string) => `[data-testid="${id}"]`;

const RUNS_TAB = testId("left-tab-runs");
const NEW_RUN_BUTTON = testId("new-run-button");
const NAME_INPUT = testId("run-name-input");
const AUTO_NAME = testId("auto-name-checkbox");
const REPO_INPUT = testId("target-repo-input");
const BROWSE_TRIGGER = testId("repo-browse-trigger");
const EXPLORER = testId("repo-browser-modal");
const REPO_VALID = testId("repo-valid");
const PIPELINE_TRIGGER = testId("pipeline-select");
const PIPELINE_OPTION = testId(`pipeline-select-option-${TUTORIAL_RUN_PIPELINE_ID}`);
/** The trigger, *and* it is holding the tutorial pipeline — one selector, so the
 *  step's condition never has to parse a display name out of a label. */
const PIPELINE_CHOSEN = `${PIPELINE_TRIGGER}[data-pipeline-id="${TUTORIAL_RUN_PIPELINE_ID}"]`;
const AGENT_TRIGGER = testId("run-agent-control");
const AGENT_POPOVER = testId("run-agent-control-popover");
/** The reserved floor profile's id is fixed forever; its NAME is renameable. */
const AGENT_DEFAULT_ROW = testId("run-agent-control-choice-default");
const AGENT_DEFAULT_CHOSEN = `${AGENT_TRIGGER}[data-choice="profile:default"]`;
const SKILLS_TRIGGER = testId("run-skill-selector");
const SKILLS_POPOVER = testId("run-skill-selector-popover");
const SKILLS_PDO_FOLDER = testId("run-skill-selector-folder-skf-pdo");
const ORCHESTRATE_ID = "pdo-orchestrate";
const INTERACTIVE_ID = "pdo-interactive";
const skillRow = (id: string) => testId(`run-skill-selector-option-${id}`);
/** The « effective » list under the trigger — one row per selected skill, whether
 *  the popover is open or not. What the checklist reads, so closing the popover
 *  after ticking does not un-tick anything (design §9). */
const skillChosen = (id: string) => testId(`run-skill-selector-row-${id}`);
const PROMPT_INPUT = testId("input-textarea");
const LAUNCH_BUTTON = testId("launch-button");
const LAUNCH_ERROR = testId("launch-error");

/** Every explorer row shares one testid; `data-entry-name` is what names this one. */
const TUTORIAL_ENTRY = `${testId("repo-browse-entry")}[data-entry-name="${TUTORIAL_REPO_NAME}"]`;

// ---- the reading half (#825) ----------------------------------------------

/** The Run inspector — the pane the node's terminal, guard and outputs live in. */
const INSPECTOR_RUN = testId("inspector-pane-run");
const TERMINAL = testId("tmux-terminal");
const RELEASE_BUTTON = testId("release-completion-btn");
const OUT_ROW = `${testId("port-row")}[data-kind="output"][data-port="${TUTORIAL_OUT_PORT}"]`;
const OUT_MODAL = `${testId("artifact-modal")}[data-port="${TUTORIAL_OUT_PORT}"]`;

/** Attribute selectors carry ids the daemon sanitised, but a quote in one would
 *  break the selector silently rather than fail loudly. */
function cssEscape(value: string): string {
  return value.replace(/["\\]/g, "\\$&");
}

/** The row of the Run this tour launched, in the left panel. */
function runRowSel(app: TourAppState): string | null {
  const id = app.latestRun?.id;
  return id ? `[data-run-row="${cssEscape(id)}"]` : null;
}

/**
 * The canvas is showing the Run this tour launched (#825). A question rather
 * than a tab-id comparison inlined twice: the step that waits on it also has to
 * *say* whether it already happened.
 */
function runIsOpen(app: TourAppState): boolean {
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
function followedNode(app: TourAppState): TourRunNode | null {
  return app.latestRun?.nodes.find((n) => n.type === "agent") ?? null;
}

/** xyflow stamps `data-id` with the node id — the one canvas target that is stable. */
function nodeSel(node: TourRunNode | null): string[] {
  return node ? [`.react-flow__node[data-id="${cssEscape(node.id)}"]`] : [];
}

/** The node stopped, but not on `completed` — failed, stopped, skipped. Said in
 *  one word, because three steps of the tour change their mind about it. */
function endedBadly(node: TourRunNode | null): boolean {
  return isNodeFinished(node) && node?.status !== "completed";
}

/** How the recap names where the node got to. A tour that was skipped early has
 *  no business claiming the node finished. */
function nodeEnding(node: TourRunNode | null): string {
  if (node == null) return "not opened";
  if (node.status === "completed") return "finished on its own";
  if (isNodeFinished(node)) return `ended \`${node.status}\``;
  return "still running";
}

/** The inspector is showing THIS node: the Run pane is up, and the canvas
 *  selection is on it. Both, because the pane survives a change of selection. */
function inspectorShows(o: TourObservation, node: TourRunNode | null): boolean {
  if (!node) return false;
  return (
    o.present(INSPECTOR_RUN) &&
    o.app.selection.kind === "node" &&
    o.app.selection.id === node.id
  );
}

function filled(o: TourObservation, selector: string): boolean {
  return (o.value(selector) ?? "").trim().length > 0;
}

/** Is this skill in the Run's own selection? */
function hasSkill(o: TourObservation, id: string): boolean {
  return o.present(skillChosen(id));
}

/**
 * How long a reading step waits for its target. Longer than the form's five
 * seconds on purpose: each of these lands right after a fetch the click itself
 * triggered — the Run's detail, the pipeline of the tab that just opened — and a
 * tour that gave up while the app was still loading what it asked for would be
 * blaming the reader for a round trip.
 */
const READING_TIMEOUT_MS = 15_000;

// ---- the steps ------------------------------------------------------------

const STEPS: TourStep[] = [
  {
    id: "open-new-run",
    title: "Open New Run",
    body: "Click New Run. A Run is one execution of a pipeline on a repository; the form that opens describes it.",
    // The button only exists on the Runs tab, so a reader who is somewhere else is
    // sent to the tab first — one step, two targets, whichever is reachable.
    target: (o) => (o.present(NEW_RUN_BUTTON) ? [NEW_RUN_BUTTON] : [RUNS_TAB]),
    waitingFor: "the New Run button",
    failureHint: "It lives at the top of the Runs tab, on the left.",
    done: (o) => o.present(NAME_INPUT),
  },
  {
    id: "name-run",
    title: "Name the Run",
    body: "Untick Auto-generated by manager, then paste the name. A name you chose is easier to find in the list than one the manager invents.",
    // Both: the field is disabled until the box is unticked, so a hole on the
    // field alone would ring a control nobody can type in.
    target: () => [NAME_INPUT, AUTO_NAME],
    waitingFor: "the Name field of the New Run form",
    failureHint: "Closing the form ends this step — reopen New Run to start again.",
    copyBlock: "my-first-run",
    confirm: true,
    // Any name the user actually typed (design Q2): the block is a suggestion,
    // and the recap will show whatever they chose.
    done: (o) => filled(o, NAME_INPUT),
  },
  {
    id: "browse-repo",
    title: "Browse for the repository",
    body: "Click the magnifier. It opens a folder explorer; the tour set it to /tmp, where the training repository was created for you.",
    target: () => [BROWSE_TRIGGER],
    waitingFor: "the magnifier of the Target repository field",
    explorerStart: TUTORIAL_REPO_PARENT,
    done: (o) => o.present(EXPLORER),
  },
  {
    id: "pick-repo",
    title: `Select ${TUTORIAL_REPO_NAME}`,
    body: `Click the ${TUTORIAL_REPO_NAME} row, then Select this folder. It is a throwaway git repository with a README and a notes.txt; nothing of yours is in it.`,
    // Soft: the rest of the explorer stays live — climbing a folder or scrolling
    // is not a mistake, and "Select this folder" is in there too. A user who
    // navigated away from `/tmp` still has a target (the dialog) instead of a
    // countdown to a failure card.
    target: (o) => {
      if (!o.present(EXPLORER)) return [BROWSE_TRIGGER];
      return o.present(TUTORIAL_ENTRY) ? [TUTORIAL_ENTRY] : [EXPLORER];
    },
    soft: true,
    waitingFor: "the folder explorer",
    failureHint: `Reopen the magnifier if the explorer was closed; the repository is at ${TUTORIAL_REPO_PATH}.`,
    // The field holds the path AND the daemon called it a git repository. Picking
    // another folder is not a failure: the condition simply does not hold, and the
    // step re-aims at the magnifier.
    done: (o) =>
      (o.value(REPO_INPUT) ?? "").trim() === TUTORIAL_REPO_PATH && o.present(REPO_VALID),
  },
  {
    id: "pick-pipeline",
    title: `Choose ${TUTORIAL_RUN_PIPELINE_ID}`,
    body: `Pick ${TUTORIAL_RUN_PIPELINE_ID}. It is a pipeline with a single node that waits for you instead of running on its own: the simplest Run there is.`,
    target: (o) => (o.present(PIPELINE_OPTION) ? [PIPELINE_OPTION] : [PIPELINE_TRIGGER]),
    soft: true,
    waitingFor: "the Pipeline menu",
    failureHint: "The menu only opens once a valid repository is selected.",
    done: (o) => o.present(PIPELINE_CHOSEN),
  },
  {
    id: "pick-profile",
    title: "Pick the Default profile",
    body: "Choose Default instead of Inherit. A profile is a harness, model and effort; the closest one wins: Node, then Run, then Project, then the instance.",
    target: (o) => (o.present(AGENT_POPOVER) ? [AGENT_DEFAULT_ROW] : [AGENT_TRIGGER]),
    soft: true,
    waitingFor: "the Agent control of the New Run form",
    done: (o) => o.present(AGENT_DEFAULT_CHOSEN),
  },
  {
    id: "add-skills",
    title: "Add the two PDO skills",
    body: "In the PDO folder, tick both skills. A skill is text added to the agent's prompt: one teaches it to drive PDO, the other to talk with you inside a node.",
    // The folder and its two children are lit as one rectangle; ticking the
    // folder's own box satisfies the step too, since it selects both.
    //
    // Which children are *there* is the user's business, though: collapsing the
    // folder, or filtering, removes the rows without removing the step's subject.
    // An `every` over the three selectors turned that into a five-second countdown
    // to "the PDO folder did not appear" while the folder sat in plain sight, so
    // the target keeps only the rows that exist — and falls back to the picker
    // itself when the filter hides the folder too, the way `pick-repo` falls back
    // to the explorer.
    target: (o) => {
      if (!o.present(SKILLS_POPOVER)) return [SKILLS_TRIGGER];
      if (!o.present(SKILLS_PDO_FOLDER)) return [SKILLS_POPOVER];
      const rows = [ORCHESTRATE_ID, INTERACTIVE_ID].map(skillRow).filter((s) => o.present(s));
      return [SKILLS_PDO_FOLDER, ...rows];
    },
    soft: true,
    waitingFor: "the PDO folder in the Skills picker",
    failureHint: "Type pdo in the picker's filter if the folder is out of view.",
    // The honest answer to "I ticked one and nothing happened": something did.
    checklist: (o) => [
      { label: ORCHESTRATE_ID, done: hasSkill(o, ORCHESTRATE_ID) },
      { label: INTERACTIVE_ID, done: hasSkill(o, INTERACTIVE_ID) },
    ],
    // On the Run's selection, not on the popover being open — closing it after
    // ticking passes the step.
    done: (o) => hasSkill(o, ORCHESTRATE_ID) && hasSkill(o, INTERACTIVE_ID),
  },
  {
    id: "write-prompt",
    title: "Give the agent a task",
    body: "Paste the prompt. It is what the node reads first; keep it small so you can watch the whole thing happen.",
    target: () => [PROMPT_INPUT],
    waitingFor: "the prompt field of the New Run form",
    copyBlock: TUTORIAL_PROMPT,
    confirm: true,
    done: (o) => filled(o, PROMPT_INPUT),
  },
  {
    id: "launch",
    title: "Launch the Run",
    body: "Click Launch. PDO creates a worktree of the training repository, starts the node in its own session, and the Run appears in the list on the left.",
    target: () => [LAUNCH_BUTTON],
    waitingFor: "the Launch button",
    // The whole tour exists for this click, so there is no Skip on it.
    // A refused launch (no harness on PATH, a missing key, no sandbox) stops the
    // tour on its own card, quoting the modal's own error line. The form is left
    // open and filled, so fixing the cause and pressing Launch again is one click.
    refused: (o) => o.text(LAUNCH_ERROR),
    refusalTitle: "The Run could not be launched",
    refusalHint:
      "Your form is still open with everything filled. Fix the cause, then press Launch again; the tour will not restart.",
    // Not "the button was clicked": a new Run in the list. Compared against the
    // count the tour started with, because the instance may well have had Runs.
    done: (o) => o.app.runCount > o.baseline.runCount,
  },

  // ---- reading the Run (#825) ---------------------------------------------
  // Launch closed the modal. From here the tour teaches the life of ONE
  // interactive node, and every step advances on the Run's own state — the
  // status the inspector polls, the guard flag, the artifact on disk — never on
  // a timer. Each is skippable: a reader who already knows this half should not
  // have to sit through it to get their checkmark.
  {
    id: "find-run",
    title: "Find your Run",
    // Launch opens the Run's tab itself, so on the nominal path the condition
    // goes true a tick after this step is entered — too late for
    // `satisfiedOnEntry`, and a step that advanced on its own was therefore
    // never seen by anybody (the FP of #825 watched the tour jump from 9 to 11).
    // It still has an idea to give — the list on the left is where your Runs
    // live — so it stops for a `Next` once the Run is open, and says what
    // already happened rather than instructing a click nobody needs to make.
    body: (o) =>
      runIsOpen(o.app)
        ? "Your Run is the top row of the list on the left, and Launch already opened its tab. Every Run is a row here; the dot is its status, blue while it runs."
        : "Click your Run at the top of the list. Every Run is a row here; the dot is its status, blue while it runs.",
    // Strictly the row. Falling back to the Runs tab would keep a target alive
    // forever, and « waiting on something that will never come » is precisely
    // what story 18 asks a tour not to do.
    target: (o) => {
      const row = runRowSel(o.app);
      return row ? [row] : [];
    },
    waitingFor: "your Run in the list on the left",
    failureHint: "The list may be filtered — clear the filters above it, or reopen the Runs tab.",
    targetTimeoutMs: READING_TIMEOUT_MS,
    skippable: true,
    advanceHint: "advances when the Run opens",
    // Only once it IS open: while the reader still has the click to make, the
    // footer says what the tour is waiting for, and a `Next` nobody can press
    // would be a control in the way of the instruction.
    confirm: (o) => runIsOpen(o.app),
    // The Run's tab is open on the canvas — which is what clicking the row does.
    done: (o) => runIsOpen(o.app),
  },
  {
    id: "open-node",
    // Opening a Run puts the selection on its live node, so on the nominal path
    // this step is entered already satisfied and stops for a `Next` — the same
    // shape as `find-run` above. It says so rather than instructing a click that
    // would change nothing, which a reader reads as an overlay swallowing their
    // click (FP finding, #825).
    title: "Open the node",
    body: (o) => {
      const name = followedNode(o.app)?.name ?? "agent";
      return inspectorShows(o, followedNode(o.app))
        ? `The inspector on the right is already showing the ${name} node — opening a Run selects the node it is running. It is the one agent of this pipeline, and that pane is its live terminal.`
        : `Click the ${name} node. It is the one agent of this pipeline; the inspector on the right shows its live terminal.`;
    },
    target: (o) => nodeSel(followedNode(o.app)),
    waitingFor: "the agent node on the canvas",
    failureHint: "It sits between Start and End; scroll or zoom the canvas to bring it into view.",
    targetTimeoutMs: READING_TIMEOUT_MS,
    skippable: true,
    advanceHint: "advances when the inspector shows the node",
    // No `confirm`: a reader who still owes the click should have it advance the
    // moment they make it. The `Next` on this card comes from `satisfiedOnEntry`,
    // which is the case the branched body above is written for.
    done: (o) => inspectorShows(o, followedNode(o.app)),
  },
  {
    id: "talk-to-agent",
    title: "Talk to the agent",
    body: "Click in the terminal and paste the line, then press Enter. This is the agent's own session; if it first asks whether you trust the folder, answer yes.",
    target: () => [TERMINAL],
    waitingFor: "the agent's terminal in the Run inspector",
    failureHint: "The terminal is in the Run tab of the inspector, above the buttons.",
    targetTimeoutMs: READING_TIMEOUT_MS,
    copyBlock: TUTORIAL_TERMINAL_LINE,
    skippable: true,
    // No condition at all: what happens in tmux is a pane of text, not a form
    // control, and a tour that claimed to read it would be guessing. `Next` is
    // live from the start — the user owns this one (story 14).
  },
  {
    id: "release-completion",
    title: "Release the completion",
    body: "Click Mark ready for completion. An interactive node is guarded: the agent cannot finish until you release it — this is the release.",
    note: '"Mark complete" next to it takes the artifacts as they are, without the agent — not for now.',
    target: () => [RELEASE_BUTTON],
    waitingFor: "the Mark ready for completion button",
    failureHint: "It only shows while the node is live, under the terminal.",
    targetTimeoutMs: READING_TIMEOUT_MS,
    skippable: true,
    advanceHint: "advances once released",
    // A node that has already stopped is past this step whatever opened its
    // guard — including the other button. Reading only the flag would stop the
    // tour on a card about a gesture that no longer has a button to make it.
    done: (o) => {
      const node = followedNode(o.app);
      return node != null && (node.released || isNodeFinished(node));
    },
  },
  {
    id: "wait-for-node",
    title: "Wait for the node to finish",
    // Nothing left to instruct once it ended badly: the checklist has shown what
    // happened and the note below says what it means.
    body: (o) =>
      endedBadly(followedNode(o.app))
        ? ""
        : "The agent writes its output and completes. Watch the status pill at the top of the inspector turn from Running to Completed.",
    // The nominal card used to open on « Nothing to do », and that is not always
    // true: an agent quick enough to run `pdo complete` before the release is
    // refused, and parks on a question — the node then ends only once somebody
    // answers it (FP finding, #825). Ordinary, not exotic, and it lands in the
    // first five minutes of a newcomer. The terminal is inside the soft zone, so
    // they can answer from where they are standing.
    note: (o) =>
      endedBadly(followedNode(o.app))
        ? "The node ended without completing. Its terminal above says why; the tour goes on to the output, which may be missing."
        : "If the agent asks you something in the terminal above, answer it — the step waits for as long as it takes.",
    // The whole inspector, as a zone rather than a target: during a wait of
    // unknown length the reader must keep the terminal — read it, scroll it,
    // answer a late question. The rest of the app stays blocked.
    target: () => [INSPECTOR_RUN],
    soft: true,
    // The one step with no limit (ticket: « sans limite de temps »). An agent
    // takes as long as it takes, and a countdown here would be the tour deciding
    // how fast somebody else's work should be.
    targetTimeoutMs: null,
    waitingFor: "the Run inspector",
    waitingNote: "Waiting for the node to finish · no time limit",
    skippable: true,
    // Skipping a wait means "I have seen enough" (design Q3). The steps after it
    // are about a file the node has not written, so the recap is the honest
    // place to land — and it says the node is still running.
    skipEndsTour: true,
    advanceHint: "advances on its own",
    // A failure is still an ending, so the step is done — but it waits for a
    // click, because the sentence explaining it is the point.
    confirm: (o) => endedBadly(followedNode(o.app)),
    done: (o) => isNodeFinished(followedNode(o.app)),
    checklist: (o) => {
      const node = followedNode(o.app);
      const finished = isNodeFinished(node);
      return [
        {
          label: "completion released",
          done: node?.released === true,
          // The engine has no « back ». A reader who skipped the release learns
          // here why nothing is happening — and the button is inside the soft
          // zone, so they can still press it.
          note:
            node?.released || finished
              ? undefined
              : "the node waits for it: click Mark ready for completion",
        },
        {
          label: "node finished",
          done: finished,
          badge: endedBadly(node) ? (node?.status ?? undefined) : undefined,
        },
      ];
    },
  },
  {
    id: "open-output",
    title: "Open the output",
    // Two beats, one card: click the row, then read what opened.
    body: (o) =>
      o.present(OUT_MODAL)
        ? "This is what the agent wrote — the summary your prompt asked for, and the only thing the next node would have received. Read it, then press Next."
        : `Click ${TUTORIAL_OUT_PORT}. It is the file the agent wrote — the summary your prompt asked for — and the only thing the next node would have received.`,
    // Re-aims onto the artifact the click opened, the way `pick-repo` re-aims
    // onto the explorer. Without it the tour ends the instant the file appears,
    // and the recap card lands on top of the summary it just told the reader to
    // open — the one thing this step exists to show them (FP finding, #825).
    target: (o) => (o.present(OUT_MODAL) ? [OUT_MODAL] : [OUT_ROW]),
    // A page to read, not a control to hit: once the artifact is up it is lit as
    // a zone — dashed, lighter dim, scrollable — and the popover moves beside it.
    soft: (o) => o.present(OUT_MODAL),
    waitingFor: `the ${TUTORIAL_OUT_PORT} row under Outputs`,
    failureHint: "A node that failed or was stopped writes no output.",
    targetTimeoutMs: READING_TIMEOUT_MS,
    skippable: true,
    advanceHint: "advances when the file opens",
    // The reader owns the last moment of the tour: once the file is up, `Next`
    // is what goes to the recap — so the recap cannot land on the summary
    // before it has been read. Until then the footer keeps its waiting line.
    confirm: (o) => o.present(OUT_MODAL),
    done: (o) => o.present(OUT_MODAL),
  },
];

export const FIRST_RUN_TOUR: TourDef = {
  id: "first-run",
  title: "First run",
  blurb: "Fill the New Run form step by step and launch a Run on a throwaway repository.",
  minutes: 4,
  steps: STEPS,
  intro: {
    title: "Launch your first Run",
    body: "You will fill the New Run form yourself, step by step: a throwaway repository, a one-node pipeline that talks to you, a profile, two skills and a prompt. Then you press Launch.",
    footnote:
      "Nothing here touches your own repositories. Both items are reused on the next run of the tour.",
    prepare: [
      {
        id: "repo",
        pending: `Training repository ${TUTORIAL_REPO_PATH}…`,
        ready: `Training repository ${TUTORIAL_REPO_PATH} ready`,
        failureTitle: "The training repository could not be created",
        run: prepareRepo,
      },
      {
        id: "pipeline",
        pending: `Pipeline ${TUTORIAL_RUN_PIPELINE_ID} in the Library…`,
        ready: `Pipeline ${TUTORIAL_RUN_PIPELINE_ID} in the Library`,
        failureTitle: "The training pipeline could not be created",
        run: prepareTutorialPipeline,
      },
    ],
  },
  recapIntro: "You ran a Run from form to output:",
  // Derived from what the tour ended up observing (#825), not from what it
  // hoped: a reader who skipped the wait is told their node is still running
  // rather than congratulated for finishing it.
  recap: (app) => {
    const node = followedNode(app);
    return [
      {
        label: app.latestRun?.name ?? "your Run",
        text: `on the training repository \`${TUTORIAL_REPO_PATH}\``,
      },
      {
        label: node?.name ?? "the node",
        text: `one interactive node, ${node?.released ? "released by you" : "guarded until a human releases it"}, ${nodeEnding(node)}`,
      },
      {
        label: TUTORIAL_OUT_PORT,
        text: "the markdown output it wrote, the only thing a next node would have received",
      },
    ];
  },
  outro: {
    // No « Open the Run » any more (#825): the tour ends inside the Run, with the
    // output open. What is left to say is that the lesson generalises — this is
    // not the tutorial's special node, it is every interactive node.
    closing:
      "The Run stays in your list. Everything you saw here — the guard, the terminal, the output — is the same on every interactive node.",
  },
};
