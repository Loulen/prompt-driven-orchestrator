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
import type { TourDef, TourObservation, TourStep } from "../tour";

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
    view: { x: 260, y: 80 }
  - id: end
    name: End
    type: end
    inputs:
      - name: result

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

function filled(o: TourObservation, selector: string): boolean {
  return (o.value(selector) ?? "").trim().length > 0;
}

/** Is this skill in the Run's own selection? */
function hasSkill(o: TourObservation, id: string): boolean {
  return o.present(skillChosen(id));
}

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
  recapIntro: "You built a Run from scratch:",
  recap: (app) => [
    {
      label: app.latestRun?.name ?? "your Run",
      text: `on the training repository \`${TUTORIAL_REPO_PATH}\``,
    },
    { label: "Pipeline", text: `\`${TUTORIAL_RUN_PIPELINE_ID}\`, one interactive node` },
    {
      label: "Agent",
      text: `profile \`Default\`, skills \`${ORCHESTRATE_ID}\` and \`${INTERACTIVE_ID}\``,
    },
  ],
  outro: {
    title: "Your Run is starting",
    primaryLabel: "Open the Run",
    // The trust prompt is named here because it is the very first thing a brand-new
    // training repository makes the harness ask, and an end card that promised « a
    // live session » and delivered a security question would read as a bug.
    closing: `The node is now a live session; your harness may first ask whether you trust \`${TUTORIAL_REPO_PATH}\`, a folder it has never seen — say yes, the tour just created it. Reading the session, and answering the agent when it waits for you, is the next tour.`,
  },
};
