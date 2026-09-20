/**
 * The *First run* tour definition (#824, extended by #825).
 *
 * Same two kinds of check as *First pipeline*. The **walk** plays the whole tour
 * against a simulated app — the New Run form, then the Run the launch created:
 * the user does exactly the gesture each step describes, and the machine must
 * reach `finished` without stalling. A step whose condition its own copy cannot
 * satisfy is caught here rather than by a person ten minutes into a real tour.
 *
 * Then the cases the walk cannot express: the two-beat targets (trigger, then the
 * option inside the popup), the wrong pick that must NOT advance, the refused
 * Launch, the unbounded wait and what it says when the node ends badly, and the
 * recap that names what the tour actually observed.
 */
import { describe, expect, it, vi } from "vitest";
import {
  canAdvance,
  confirmStep,
  currentStep,
  needsConfirm,
  observeTour,
  recapOf,
  skipStep,
  startTour,
  stepBody,
  stepNote,
  stepSoft,
  type TourAppState,
  type TourObservation,
  type TourRun,
  type TourRunNode,
  type TourStep,
} from "../tour";
import {
  FIRST_RUN_TOUR,
  TUTORIAL_PROMPT,
  TUTORIAL_REPO_PATH,
  TUTORIAL_RUN_PIPELINE_ID,
  TUTORIAL_TERMINAL_LINE,
} from "./firstRun";
import { CARD_HEIGHT, CARD_WIDTH } from "../nodePlacement";

const TOUR = FIRST_RUN_TOUR;
const STEPS = TOUR.steps;

const t = (id: string) => `[data-testid="${id}"]`;

const RUNS_TAB = t("left-tab-runs");
const NEW_RUN = t("new-run-button");
const NAME_INPUT = t("run-name-input");
const AUTO_NAME = t("auto-name-checkbox");
const REPO_INPUT = t("target-repo-input");
const BROWSE = t("repo-browse-trigger");
const EXPLORER = t("repo-browser-modal");
const ENTRY = `${t("repo-browse-entry")}[data-entry-name="pdo-tutorial"]`;
const REPO_VALID = t("repo-valid");
const PIPELINE_TRIGGER = t("pipeline-select");
const PIPELINE_CHOSEN = `${PIPELINE_TRIGGER}[data-pipeline-id="${TUTORIAL_RUN_PIPELINE_ID}"]`;
const PIPELINE_OPTION = t(`pipeline-select-option-${TUTORIAL_RUN_PIPELINE_ID}`);
const AGENT_TRIGGER = t("run-agent-control");
const AGENT_POPOVER = t("run-agent-control-popover");
const AGENT_DEFAULT_ROW = t("run-agent-control-choice-default");
const AGENT_DEFAULT_CHOSEN = `${AGENT_TRIGGER}[data-choice="profile:default"]`;
const SKILLS_TRIGGER = t("run-skill-selector");
const SKILLS_POPOVER = t("run-skill-selector-popover");
const SKILLS_FOLDER = t("run-skill-selector-folder-skf-pdo");
const skillOption = (id: string) => t(`run-skill-selector-option-${id}`);
const skillChosen = (id: string) => t(`run-skill-selector-row-${id}`);
const PROMPT_INPUT = t("input-textarea");
// #825 — the reading half: the Run, its node, and what the node produced.
const NODE_ID = "assistant";
const NODE_CARD = `.react-flow__node[data-id="${NODE_ID}"]`;
const INSPECTOR_RUN = t("inspector-pane-run");
const TERMINAL = t("tmux-terminal");
const RELEASE = t("release-completion-btn");
const OUT_ROW = `${t("port-row")}[data-kind="output"][data-port="out"]`;
const OUT_MODAL = `${t("artifact-modal")}[data-port="out"]`;
const LAUNCH = t("launch-button");
const LAUNCH_ERROR = t("launch-error");

const EMPTY_APP: TourAppState = {
  pipelineId: null,
  pipeline: null,
  prompts: {},
  selection: { kind: "none", id: null, edgeIndex: null },
  dirty: false,
  libraryPipelineIds: [],
  runCount: 0,
  latestRun: null,
  activeRunId: null,
};

// ---- the simulator --------------------------------------------------------

/**
 * A stand-in for the app around the New Run form: which elements are on screen,
 * what the fields hold, and the Runs the instance has. The gestures below move it
 * the way the real UI does — opening the modal shows its fields, picking a folder
 * fills the repo field and validates it, Launch adds a Run and closes the modal.
 *
 * It starts with **two Runs already in the list**: this instance is not empty, so
 * a last step that merely asked "is there a Run?" would be satisfied before the
 * user pressed anything.
 */
class FakeApp {
  app: TourAppState = { ...EMPTY_APP, runCount: 2, latestRun: { id: "r-old", name: "older", nodes: [] } };
  /** Frozen at construction — what "did this tour cause it?" is measured against. */
  readonly baseline: TourAppState = { ...this.app };
  private dom = new Set<string>([RUNS_TAB, NEW_RUN]);
  private values: Record<string, string> = {};
  private texts: Record<string, string> = {};

  obs(): TourObservation {
    return {
      app: this.app,
      baseline: this.baseline,
      present: (selector) => this.dom.has(selector),
      value: (selector) => this.values[selector] ?? null,
      text: (selector) => this.texts[selector] ?? null,
    };
  }

  show(...selectors: string[]) {
    for (const s of selectors) this.dom.add(s);
  }
  hide(...selectors: string[]) {
    for (const s of selectors) this.dom.delete(s);
  }
  type(selector: string, value: string) {
    this.values[selector] = value;
  }
  say(selector: string, text: string) {
    this.texts[selector] = text;
    this.dom.add(selector);
  }

  openModal() {
    this.hide(NEW_RUN);
    this.show(NAME_INPUT, AUTO_NAME, REPO_INPUT, BROWSE, PIPELINE_TRIGGER, AGENT_TRIGGER, SKILLS_TRIGGER, PROMPT_INPUT, LAUNCH);
  }

  /** The explorer's own dialog, listing the training repo's row. */
  openExplorer() {
    this.show(EXPLORER, ENTRY);
  }

  pickRepo(path: string, valid: boolean) {
    this.hide(EXPLORER, ENTRY);
    this.type(REPO_INPUT, path);
    if (valid) this.show(REPO_VALID);
    else this.hide(REPO_VALID);
  }

  openPipelineMenu() {
    this.show(PIPELINE_OPTION);
  }
  choosePipeline(id: string) {
    this.hide(PIPELINE_OPTION);
    if (id === TUTORIAL_RUN_PIPELINE_ID) this.show(PIPELINE_CHOSEN);
    else this.hide(PIPELINE_CHOSEN);
  }

  openAgentPopover() {
    this.show(AGENT_POPOVER, AGENT_DEFAULT_ROW);
  }
  chooseProfile(id: string) {
    this.hide(AGENT_POPOVER, AGENT_DEFAULT_ROW);
    if (id === "default") this.show(AGENT_DEFAULT_CHOSEN);
    else this.hide(AGENT_DEFAULT_CHOSEN);
  }

  openSkillsPopover() {
    this.show(SKILLS_POPOVER, SKILLS_FOLDER, skillOption("pdo-orchestrate"), skillOption("pdo-interactive"));
  }
  tickSkill(id: string) {
    this.show(skillChosen(id));
  }
  closeSkillsPopover() {
    this.hide(SKILLS_POPOVER, SKILLS_FOLDER, skillOption("pdo-orchestrate"), skillOption("pdo-interactive"));
  }
  /** The folder's chevron: its rows go, the folder itself stays on screen. */
  collapseSkillsFolder() {
    this.hide(skillOption("pdo-orchestrate"), skillOption("pdo-interactive"));
  }
  /** A filter that matches nothing: even the PDO folder leaves the tree. */
  filterSkillsAway() {
    this.hide(SKILLS_FOLDER, skillOption("pdo-orchestrate"), skillOption("pdo-interactive"));
  }

  /** A launch the daemon accepted: the modal closes itself, the Run is in the list. */
  launch(name = "my-first-run") {
    this.hide(NAME_INPUT, AUTO_NAME, REPO_INPUT, BROWSE, PIPELINE_TRIGGER, AGENT_TRIGGER, SKILLS_TRIGGER, PROMPT_INPUT, LAUNCH);
    this.app = {
      ...this.app,
      runCount: this.app.runCount + 1,
      // The list entry only. Its nodes arrive with the detail, which the UI
      // fetches when the Run is opened — which is what step 10 asks for.
      latestRun: { id: "r-new", name, nodes: [] },
    };
    this.show(runRow("r-new"));
  }

  /** A launch the daemon refused: the modal stays, with its error line showing. */
  refuseLaunch(reason: string) {
    this.say(LAUNCH_ERROR, reason);
  }

  // ---- the reading half (#825) --------------------------------------------

  /** Clicking the Run's row: its tab opens on the canvas, and the detail lands
   *  with the node the tour is about to talk about. */
  openRun() {
    this.show(NODE_CARD);
    this.app = {
      ...this.app,
      activeRunId: this.app.latestRun?.id ?? null,
      latestRun: this.app.latestRun
        ? {
            ...this.app.latestRun,
            nodes: [{ id: NODE_ID, name: NODE_ID, type: "agent", status: "running", released: false }],
          }
        : null,
    };
  }

  /** Clicking the node card: the Run pane of the inspector takes over, with the
   *  live terminal and the completion guard's buttons in it. */
  selectNode() {
    this.show(INSPECTOR_RUN, TERMINAL, RELEASE);
    this.app = { ...this.app, selection: { kind: "node", id: NODE_ID, edgeIndex: null } };
  }

  releaseCompletion() {
    this.patchNode({ released: true });
  }

  /** The node stops. Only a completed one leaves an output row behind. */
  finishNode(status = "completed") {
    this.hide(RELEASE);
    this.patchNode({ status });
    if (status === "completed") this.show(OUT_ROW);
  }

  openOutput() {
    this.show(OUT_MODAL);
  }

  private patchNode(patch: Partial<TourRunNode>) {
    const run = this.app.latestRun;
    if (!run) return;
    this.app = {
      ...this.app,
      latestRun: { ...run, nodes: run.nodes.map((n) => ({ ...n, ...patch })) },
    };
  }
}

const runRow = (id: string) => `[data-run-row="${id}"]`;

/** What the user does at each step. Keys are checked against the definition, so a
 *  new step with no gesture fails loudly instead of being walked past. */
function gestures(fake: FakeApp): Record<string, () => void> {
  return {
    "open-new-run": () => fake.openModal(),
    "name-run": () => fake.type(NAME_INPUT, "my-first-run"),
    "browse-repo": () => fake.openExplorer(),
    "pick-repo": () => fake.pickRepo(TUTORIAL_REPO_PATH, true),
    "pick-pipeline": () => {
      fake.openPipelineMenu();
      fake.choosePipeline(TUTORIAL_RUN_PIPELINE_ID);
    },
    "pick-profile": () => {
      fake.openAgentPopover();
      fake.chooseProfile("default");
    },
    "add-skills": () => {
      fake.openSkillsPopover();
      fake.tickSkill("pdo-orchestrate");
      fake.tickSkill("pdo-interactive");
      fake.closeSkillsPopover();
    },
    "write-prompt": () => fake.type(PROMPT_INPUT, TUTORIAL_PROMPT),
    launch: () => fake.launch(),
    "find-run": () => fake.openRun(),
    "open-node": () => fake.selectNode(),
    // Nothing observable: what happens in tmux is a pane of text. The walk's
    // `Next` is the gesture.
    "talk-to-agent": () => {},
    "release-completion": () => fake.releaseCompletion(),
    "wait-for-node": () => fake.finishNode("completed"),
    "open-output": () => fake.openOutput(),
  };
}

/** Drive the machine the way the host does, one step at a time. */
function walk(fake: FakeApp, script: Record<string, () => void>) {
  let run = startTour(TOUR, fake.obs());
  // A tour with an intro opens on its card; the walk is about the steps.
  expect(run.phase).toBe("intro");
  run = { ...run, phase: "running" as const };
  let clock = 0;
  const visited: string[] = [];

  for (let guard = 0; guard < STEPS.length * 3 && run.phase === "running"; guard++) {
    const step = currentStep(TOUR, run)!;
    if (visited.at(-1) !== step.id) visited.push(step.id);

    const gesture = script[step.id];
    expect(gesture, `no gesture scripted for step "${step.id}"`).toBeDefined();
    gesture();

    clock += 200;
    run = observeTour(TOUR, run, fake.obs(), clock);
    if (run.phase !== "running" || currentStep(TOUR, run)?.id !== step.id) continue;

    if (needsConfirm(TOUR, run, fake.obs())) {
      expect(canAdvance(TOUR, run, fake.obs()), `Next stuck on "${step.id}"`).toBe(true);
      run = confirmStep(TOUR, run, fake.obs());
    } else if (step.skippable) {
      run = skipStep(TOUR, run, fake.obs());
    }
    expect(currentStep(TOUR, run)?.id, `stalled on "${step.id}"`).not.toBe(step.id);
  }
  return { run, visited };
}

// ---- the walk -------------------------------------------------------------

describe("walking the whole tour", () => {
  it("reaches the end when the user performs the gesture each step asks for", () => {
    const fake = new FakeApp();
    const { run, visited } = walk(fake, gestures(fake));

    expect(run.phase).toBe("finished");
    expect(visited).toEqual(STEPS.map((s) => s.id));
  });

  it("produces the Run the ticket describes", () => {
    const fake = new FakeApp();
    const script = gestures(fake);
    for (const step of STEPS) script[step.id]();

    // The training repo, the tutorial pipeline, the Default profile, both skills.
    expect(fake.obs().value(REPO_INPUT)).toBe(TUTORIAL_REPO_PATH);
    expect(fake.obs().present(PIPELINE_CHOSEN)).toBe(true);
    expect(fake.obs().present(AGENT_DEFAULT_CHOSEN)).toBe(true);
    expect(fake.obs().present(skillChosen("pdo-orchestrate"))).toBe(true);
    expect(fake.obs().present(skillChosen("pdo-interactive"))).toBe(true);
    expect(fake.obs().value(PROMPT_INPUT)).toBe(TUTORIAL_PROMPT);
    expect(fake.app.runCount).toBe(3);
  });
});

// ---- the targets ----------------------------------------------------------

const stepById = (id: string) => STEPS.find((s) => s.id === id)!;

describe("targets that re-aim as the user works", () => {
  it("sends a reader on another left tab to the Runs tab first", () => {
    const fake = new FakeApp();
    fake.hide(NEW_RUN);
    expect(stepById("open-new-run").target(fake.obs())).toEqual([RUNS_TAB]);
    fake.show(NEW_RUN);
    expect(stepById("open-new-run").target(fake.obs())).toEqual([NEW_RUN]);
  });

  it.each([
    ["pick-pipeline", PIPELINE_TRIGGER, () => {}, PIPELINE_OPTION],
    ["pick-profile", AGENT_TRIGGER, () => {}, AGENT_DEFAULT_ROW],
  ])("points at the trigger of %s until its popup is open", (id, trigger, _open, option) => {
    const fake = new FakeApp();
    fake.openModal();
    expect(stepById(id).target(fake.obs())).toEqual([trigger]);
    if (id === "pick-pipeline") fake.openPipelineMenu();
    else fake.openAgentPopover();
    expect(stepById(id).target(fake.obs())).toEqual([option]);
  });

  it("lights the PDO folder and both of its skills as one rectangle", () => {
    const fake = new FakeApp();
    fake.openModal();
    expect(stepById("add-skills").target(fake.obs())).toEqual([SKILLS_TRIGGER]);
    fake.openSkillsPopover();
    expect(stepById("add-skills").target(fake.obs())).toEqual([
      SKILLS_FOLDER,
      skillOption("pdo-orchestrate"),
      skillOption("pdo-interactive"),
    ]);
  });

  /**
   * Collapsing the folder is not a mistake, and it is the one gesture that takes
   * the two rows off screen while leaving the step's subject in plain sight. An
   * `every` over the three selectors read that as "the target is gone" and
   * counted down to a failure card the user could not understand.
   */
  it("keeps pointing at the PDO folder when its rows are collapsed away", () => {
    const fake = new FakeApp();
    fake.openModal();
    fake.openSkillsPopover();
    fake.collapseSkillsFolder();

    const target = stepById("add-skills").target(fake.obs());
    expect(target).toEqual([SKILLS_FOLDER]);
    // …and it is a target the machine can still see, which is the whole point.
    expect(target.every((s) => fake.obs().present(s))).toBe(true);
  });

  it("falls back to the picker when a filter hides the PDO folder too", () => {
    const fake = new FakeApp();
    fake.openModal();
    fake.openSkillsPopover();
    fake.filterSkillsAway();
    expect(stepById("add-skills").target(fake.obs())).toEqual([SKILLS_POPOVER]);
  });

  it("does not stop the tour while the folder stays collapsed", () => {
    const fake = new FakeApp();
    fake.openModal();
    let run = startTour(TOUR, fake.obs());
    run = {
      ...run,
      phase: "running" as const,
      index: STEPS.findIndex((s) => s.id === "add-skills"),
    };
    fake.openSkillsPopover();
    fake.collapseSkillsFolder();

    // Well past the 5 s the engine gives a missing target.
    for (const clock of [100, 3_000, 9_000, 20_000]) {
      run = observeTour(TOUR, run, fake.obs(), clock);
    }
    expect(run.phase).toBe("running");
    expect(currentStep(TOUR, run)?.id).toBe("add-skills");
  });

  /**
   * The explorer is a soft zone, and a user who climbed out of `/tmp` has no
   * `pdo-tutorial` row to ring. The dialog itself is the fallback target, because
   * the alternative — an empty target list — starts a countdown to a failure card
   * on someone who is only looking around.
   */
  it("falls back to the explorer dialog when the training repo is out of view", () => {
    const fake = new FakeApp();
    fake.openModal();
    expect(stepById("pick-repo").target(fake.obs())).toEqual([BROWSE]);
    fake.openExplorer();
    expect(stepById("pick-repo").target(fake.obs())).toEqual([ENTRY]);
    fake.hide(ENTRY);
    expect(stepById("pick-repo").target(fake.obs())).toEqual([EXPLORER]);
  });

  it("asks the explorer to open on /tmp", () => {
    expect(stepById("browse-repo").explorerStart).toBe("/tmp");
    // Exactly one step carries the override: it is the magnifier's step or nothing.
    expect(STEPS.filter((s) => s.explorerStart)).toHaveLength(1);
  });
});

// ---- the wrong gesture ----------------------------------------------------

describe("a wrong choice does not advance the tour", () => {
  it("refuses another folder than the training repository", () => {
    const fake = new FakeApp();
    fake.openModal();
    const step = stepById("pick-repo");
    fake.pickRepo("/home/someone/real-project", true);
    expect(step.done!(fake.obs())).toBe(false);
    // …and a path that is right but not a repository is not enough either.
    fake.pickRepo(TUTORIAL_REPO_PATH, false);
    expect(step.done!(fake.obs())).toBe(false);
    fake.pickRepo(TUTORIAL_REPO_PATH, true);
    expect(step.done!(fake.obs())).toBe(true);
  });

  it("refuses another pipeline, and another profile", () => {
    const fake = new FakeApp();
    fake.openModal();
    fake.choosePipeline("some-other-pipeline");
    expect(stepById("pick-pipeline").done!(fake.obs())).toBe(false);
    fake.chooseProfile("inherit");
    expect(stepById("pick-profile").done!(fake.obs())).toBe(false);
  });

  it("waits for BOTH skills, and says which one is still missing", () => {
    const fake = new FakeApp();
    const step = stepById("add-skills");
    fake.openModal();
    fake.openSkillsPopover();

    expect(step.done!(fake.obs())).toBe(false);
    expect(step.checklist!(fake.obs())).toEqual([
      { label: "pdo-orchestrate", done: false },
      { label: "pdo-interactive", done: false },
    ]);

    fake.tickSkill("pdo-interactive");
    expect(step.done!(fake.obs())).toBe(false);
    expect(step.checklist!(fake.obs())).toEqual([
      { label: "pdo-orchestrate", done: false },
      { label: "pdo-interactive", done: true },
    ]);

    fake.tickSkill("pdo-orchestrate");
    expect(step.done!(fake.obs())).toBe(true);
    // Order is free, and closing the popover afterwards does not un-tick anything:
    // the condition reads the Run's selection, not the picker being open.
    fake.closeSkillsPopover();
    expect(step.done!(fake.obs())).toBe(true);
  });

  /**
   * An instance that already delivers both PDO skills to every Run shows them
   * ticked AND disabled — there is no gesture left to make. The step reads the
   * Run's *effective* skills, so it is satisfied on entry: the popover then waits
   * for an explicit `Next` (engine rule) instead of flashing past the one thing
   * this step has to give, which is the explanation.
   */
  it("counts a skill the Run inherits, since the user cannot tick it", () => {
    const fake = new FakeApp();
    fake.openModal();
    // Inherited rows land in the same effective list as the ticked ones.
    fake.tickSkill("pdo-orchestrate");
    fake.tickSkill("pdo-interactive");

    // Entered through the machine's own path (the step before it completing), so
    // `satisfiedOnEntry` is computed rather than assumed.
    let run = startTour(TOUR, fake.obs());
    run = {
      ...run,
      phase: "running" as const,
      index: STEPS.findIndex((s) => s.id === "pick-profile"),
    };
    fake.chooseProfile("default");
    run = observeTour(TOUR, run, fake.obs(), 100);

    expect(currentStep(TOUR, run)?.id).toBe("add-skills");
    expect(run.satisfiedOnEntry).toBe(true);
    expect(needsConfirm(TOUR, run, fake.obs())).toBe(true);
    expect(canAdvance(TOUR, run, fake.obs())).toBe(true);

    // And it does NOT flash past: another tick leaves it exactly where it is.
    expect(currentStep(TOUR, observeTour(TOUR, run, fake.obs(), 300))?.id).toBe("add-skills");
  });
});

// ---- the last step --------------------------------------------------------

describe("the Launch step", () => {
  it("is not skippable — the whole tour exists for that click", () => {
    expect(stepById("launch").skippable).toBeFalsy();
  });

  /**
   * On an instance that already has Runs, "a Run exists" is true before the user
   * touches anything. The condition is against the count the tour STARTED with.
   */
  it("advances on a Run this tour caused, not on any Run at all", () => {
    const fake = new FakeApp();
    const step = stepById("launch");
    expect(fake.app.runCount).toBe(2);
    expect(step.done!(fake.obs())).toBe(false);

    fake.launch();
    expect(step.done!(fake.obs())).toBe(true);
  });

  it("stops the tour on a refusal, quoting the daemon verbatim", () => {
    const fake = new FakeApp();
    const script = gestures(fake);
    for (const id of ["open-new-run", "name-run", "browse-repo", "pick-repo", "pick-pipeline", "pick-profile", "add-skills", "write-prompt"]) {
      script[id]();
    }

    let run = startTour(TOUR, fake.obs());
    run = { ...run, phase: "running" as const, index: STEPS.findIndex((s) => s.id === "launch") };

    fake.refuseLaunch('harness "claude" not found on PATH');
    run = observeTour(TOUR, run, fake.obs(), 1_000);

    expect(run.phase).toBe("failed");
    expect(run.failure).toMatchObject({
      kind: "refused",
      stepId: "launch",
      stepNumber: STEPS.findIndex((s) => s.id === "launch") + 1,
      reason: 'harness "claude" not found on PATH',
      title: "The Run could not be launched",
    });
    // Not finished, so nothing marks the tour done — and the failure card's hint
    // tells the user their form is still there.
    expect(run.failure?.hint).toContain("still open");
  });

  /**
   * The refusal is read BEFORE the condition. Otherwise a stale error line next to
   * a Run that did start would stop a tour that actually succeeded — and, the
   * other way round, a refusal could be walked past by any predicate that went
   * true in the same tick.
   */
  it("prefers the refusal to the condition when both hold", () => {
    const fake = new FakeApp();
    let run = startTour(TOUR, fake.obs());
    run = { ...run, phase: "running" as const, index: STEPS.findIndex((s) => s.id === "launch") };

    fake.launch();
    fake.refuseLaunch("sandbox unavailable");
    run = observeTour(TOUR, run, fake.obs(), 1_000);

    expect(run.phase).toBe("failed");
    expect(run.failure?.reason).toBe("sandbox unavailable");
  });
});

// ---- reading the Run (#825) -----------------------------------------------

/** Put the machine on a named step, with the app already where that step starts. */
function at(fake: FakeApp, id: string): TourRun {
  const run = startTour(TOUR, fake.obs());
  return { ...run, phase: "running", index: STEPS.findIndex((s) => s.id === id) };
}

describe("finding the Run, and opening its node", () => {
  it("points at the row of the Run this tour launched, and advances when its tab opens", () => {
    const fake = new FakeApp();
    const step = stepById("find-run");
    fake.launch();

    expect(step.target(fake.obs())).toEqual([runRow("r-new")]);
    expect(step.done!(fake.obs())).toBe(false);
    fake.openRun();
    expect(step.done!(fake.obs())).toBe(true);
  });

  /**
   * Launch opens the Run's tab itself, a tick after this step is entered — so
   * `satisfiedOnEntry` is false and a step that advanced on its own was never
   * shown to anybody (the FP of #825 watched the tour jump from 9 to 11). It
   * asks for an explicit `Next` instead, and says what already happened rather
   * than instructing a click nobody needs to make.
   */
  it("shows its card even when Launch already opened the Run", () => {
    const fake = new FakeApp();
    let run = at(fake, "launch");
    fake.launch();
    run = observeTour(TOUR, run, fake.obs(), 100);
    expect(currentStep(TOUR, run)?.id).toBe("find-run");

    // The app catches up a tick later: the tab opens by itself.
    fake.openRun();
    run = observeTour(TOUR, run, fake.obs(), 200);

    expect(currentStep(TOUR, run)?.id).toBe("find-run");
    expect(needsConfirm(TOUR, run, fake.obs())).toBe(true);
    expect(canAdvance(TOUR, run, fake.obs())).toBe(true);
    expect(stepBody(stepById("find-run"), fake.obs())).toContain("already opened its tab");
    // …and only the reader's click moves it on.
    expect(currentStep(TOUR, confirmStep(TOUR, run, fake.obs()))?.id).toBe("open-node");
  });

  it("asks for the click when the reader is somewhere else", () => {
    const fake = new FakeApp();
    fake.launch();
    const run = at(fake, "find-run");
    expect(stepBody(stepById("find-run"), fake.obs())).toContain("Click your Run");
    expect(canAdvance(TOUR, run, fake.obs())).toBe(false);
    // …and no dead `Next` in the way of the instruction: the footer keeps the
    // waiting line the design drew, until there is something to confirm.
    expect(needsConfirm(TOUR, run, fake.obs())).toBe(false);
    expect(stepById("find-run").advanceHint).toContain("advances");
  });

  /**
   * « Open the session » is two gestures a newcomer does not know: the Run is a
   * row, the node is where the agent lives. Neither is the run list's own
   * `open-session-button`, which opens a bash shell on a FINISHED run — the wrong
   * thing, and absent while the node runs.
   */
  it("aims at the node card on the canvas, never at the run list's shell button", () => {
    const fake = new FakeApp();
    const step = stepById("open-node");
    fake.launch();
    // Before the detail lands there is no node to aim at, and no claim to make.
    expect(step.target(fake.obs())).toEqual([]);
    fake.openRun();
    expect(step.target(fake.obs())).toEqual([NODE_CARD]);
    expect(step.target(fake.obs())).not.toContain(t("open-session-button"));
  });

  /** The Run pane can be up for another node entirely: both halves are required. */
  it("waits for the inspector to show THIS node", () => {
    const fake = new FakeApp();
    const step = stepById("open-node");
    fake.launch();
    fake.openRun();
    expect(step.done!(fake.obs())).toBe(false);

    fake.selectNode();
    expect(step.done!(fake.obs())).toBe(true);

    fake.app = { ...fake.app, selection: { kind: "node", id: "someone-else", edgeIndex: null } };
    expect(step.done!(fake.obs())).toBe(false);
  });

  /**
   * Opening the Run selects the node it is running, so this step is usually
   * entered already satisfied — and a card that instructs a click which changes
   * nothing reads as an overlay that swallowed it. Same shape, same answer as
   * `find-run`: say what already happened (FP finding, #825).
   */
  it("says the inspector is already on the node instead of asking for a dead click", () => {
    const fake = new FakeApp();
    let run = at(fake, "find-run");
    fake.launch();
    fake.openRun();
    fake.selectNode();
    run = observeTour(TOUR, run, fake.obs(), 100);
    run = confirmStep(TOUR, run, fake.obs());

    expect(currentStep(TOUR, run)?.id).toBe("open-node");
    // Entered satisfied: it stops for a `Next` rather than flashing past…
    expect(run.satisfiedOnEntry).toBe(true);
    expect(needsConfirm(TOUR, run, fake.obs())).toBe(true);
    // …and the card describes the state instead of ordering a click.
    const body = stepBody(stepById("open-node"), fake.obs());
    expect(body).toContain("already showing");
    expect(body).not.toContain("Click the");
  });

  it("asks for the click while the inspector is not on the node", () => {
    const fake = new FakeApp();
    fake.launch();
    fake.openRun();
    const run = at(fake, "open-node");
    expect(stepBody(stepById("open-node"), fake.obs())).toContain("Click the");
    expect(canAdvance(TOUR, run, fake.obs())).toBe(false);
    // No `confirm` of its own: the click the card asks for is what advances it.
    expect(needsConfirm(TOUR, run, fake.obs())).toBe(false);
  });

  it("names the node the Run actually has, rather than assuming one", () => {
    const fake = new FakeApp();
    fake.launch();
    fake.openRun();
    fake.app = {
      ...fake.app,
      latestRun: {
        ...fake.app.latestRun!,
        nodes: [{ id: "n1", name: "reviewer", type: "agent", status: "running", released: false }],
      },
    };
    expect(stepBody(stepById("open-node"), fake.obs())).toContain("reviewer");
  });
});

describe("the completion guard", () => {
  it("advances on the release flag, not on the click", () => {
    const fake = new FakeApp();
    const step = stepById("release-completion");
    fake.launch();
    fake.openRun();
    fake.selectNode();

    expect(step.target(fake.obs())).toEqual([RELEASE]);
    expect(step.done!(fake.obs())).toBe(false);
    fake.releaseCompletion();
    expect(step.done!(fake.obs())).toBe(true);
  });

  /**
   * « Mark complete » is the other button, and it ends the node without ever
   * setting the flag. Reading only the flag would strand the tour on a card about
   * a gesture whose button is no longer on screen.
   */
  it("treats a node that stopped as past this step, whatever opened its guard", () => {
    const fake = new FakeApp();
    fake.launch();
    fake.openRun();
    fake.selectNode();
    fake.finishNode("completed");
    expect(stepById("release-completion").done!(fake.obs())).toBe(true);
  });

  it("names the look-alike button it is NOT about", () => {
    expect(stepNote(stepById("release-completion"), new FakeApp().obs())).toContain(
      "Mark complete",
    );
  });
});

describe("waiting for the node to finish", () => {
  const waitStep = () => stepById("wait-for-node");

  /** The ticket's « sans limite de temps »: an agent takes as long as it takes. */
  it("never gives up, however long the node runs", () => {
    const fake = new FakeApp();
    fake.launch();
    fake.openRun();
    fake.selectNode();
    fake.releaseCompletion();
    let run = at(fake, "wait-for-node");
    // The inspector is on screen throughout, and a whole hour passes.
    for (const clock of [100, 5_000, 60_000, 3_600_000]) {
      run = observeTour(TOUR, run, fake.obs(), clock);
    }
    expect(run.phase).toBe("running");
    expect(currentStep(TOUR, run)?.id).toBe("wait-for-node");
    expect(waitStep().targetTimeoutMs).toBeNull();
  });

  /**
   * The card used to open on « Nothing to do », and that is a lie in an ordinary
   * race: an agent that called `pdo complete` before the release was refused, and
   * parks on a question — the node then ends only once somebody answers it. The
   * terminal is inside the soft zone, so the reader can (FP finding, #825).
   */
  it("tells the reader to answer the agent instead of promising there is nothing to do", () => {
    const fake = new FakeApp();
    fake.launch();
    fake.openRun();
    fake.selectNode();
    fake.releaseCompletion();

    expect(stepBody(waitStep(), fake.obs())).not.toContain("Nothing to do");
    expect(stepNote(waitStep(), fake.obs())).toContain("answer it");
  });

  it("lights the whole inspector as a zone to roam in", () => {
    const fake = new FakeApp();
    fake.launch();
    fake.openRun();
    fake.selectNode();
    expect(waitStep().target(fake.obs())).toEqual([INSPECTOR_RUN]);
    expect(waitStep().soft).toBe(true);
    expect(waitStep().waitingNote).toContain("no time limit");
  });

  it("slides on by itself when the node completes", () => {
    const fake = new FakeApp();
    fake.launch();
    fake.openRun();
    fake.selectNode();
    fake.releaseCompletion();
    let run = at(fake, "wait-for-node");
    fake.finishNode("completed");
    run = observeTour(TOUR, run, fake.obs(), 1_000);
    expect(currentStep(TOUR, run)?.id).toBe("open-output");
  });

  /**
   * A failure is still an ending, so the step is done — but it stops for a click,
   * because the sentence explaining what happened is the only thing it has left
   * to give.
   */
  it("stops for a Next when the node ended without completing", () => {
    const fake = new FakeApp();
    fake.launch();
    fake.openRun();
    fake.selectNode();
    fake.releaseCompletion();
    let run = at(fake, "wait-for-node");
    fake.finishNode("failed");
    run = observeTour(TOUR, run, fake.obs(), 1_000);

    expect(currentStep(TOUR, run)?.id).toBe("wait-for-node");
    expect(needsConfirm(TOUR, run, fake.obs())).toBe(true);
    expect(canAdvance(TOUR, run, fake.obs())).toBe(true);
    // The instruction gives way to the explanation.
    expect(stepBody(waitStep(), fake.obs())).toBe("");
    expect(stepNote(waitStep(), fake.obs())).toContain("ended without completing");
    expect(waitStep().checklist!(fake.obs())[1]).toMatchObject({ done: true, badge: "failed" });
  });

  /**
   * The engine has no « back ». A reader who skipped the release would otherwise
   * sit in front of a spinner that can never end — so the checklist says, in
   * place, what the node is waiting for. The button is inside the soft zone.
   */
  it("says in the checklist why nothing is happening after a skipped release", () => {
    const fake = new FakeApp();
    fake.launch();
    fake.openRun();
    fake.selectNode();

    const [released, finished] = waitStep().checklist!(fake.obs());
    expect(released).toMatchObject({ done: false });
    expect(released.note).toContain("Mark ready for completion");
    expect(finished).toMatchObject({ done: false, badge: undefined });

    fake.releaseCompletion();
    expect(waitStep().checklist!(fake.obs())[0]).toMatchObject({ done: true, note: undefined });
  });

  /** Skipping a wait means "I have seen enough" (design Q3): the steps after it
   *  are about a file the node has not written. */
  it("ends the tour on Skip, rather than pointing at an output nobody wrote", () => {
    const fake = new FakeApp();
    fake.launch();
    fake.openRun();
    fake.selectNode();
    const run = skipStep(TOUR, at(fake, "wait-for-node"), fake.obs());
    expect(run.phase).toBe("finished");
  });
});

describe("opening the output", () => {
  it("points at the out row under Outputs, and is satisfied when the file opens", () => {
    const fake = new FakeApp();
    const step = stepById("open-output");
    fake.launch();
    fake.openRun();
    fake.selectNode();
    fake.finishNode("completed");

    expect(step.target(fake.obs())).toEqual([OUT_ROW]);
    expect(step.done!(fake.obs())).toBe(false);
    fake.openOutput();
    expect(step.done!(fake.obs())).toBe(true);
  });

  /**
   * The step ends the tour, and the recap card is centred — so a step that
   * advanced the instant the artifact appeared put the card straight on top of
   * the summary it had just told the reader to open (FP finding, #825). It
   * re-aims onto the artifact instead: lit as a zone to read, popover beside it,
   * and the reader presses `Next` when they are done.
   */
  it("re-aims onto the artifact and lets the reader read it before the recap", () => {
    const fake = new FakeApp();
    const step = stepById("open-output");
    fake.launch();
    fake.openRun();
    fake.selectNode();
    fake.finishNode("completed");

    expect(stepSoft(step, fake.obs())).toBe(false);
    let run = at(fake, "open-output");
    // Before the click the footer is the waiting line, not a dead button.
    expect(needsConfirm(TOUR, run, fake.obs())).toBe(false);
    fake.openOutput();
    run = observeTour(TOUR, run, fake.obs(), 1_000);

    expect(currentStep(TOUR, run)?.id).toBe("open-output");
    expect(step.target(fake.obs())).toEqual([OUT_MODAL]);
    expect(stepSoft(step, fake.obs())).toBe(true);
    expect(stepBody(step, fake.obs())).toContain("Read it");
    expect(needsConfirm(TOUR, run, fake.obs())).toBe(true);
    expect(canAdvance(TOUR, run, fake.obs())).toBe(true);
    expect(confirmStep(TOUR, run, fake.obs()).phase).toBe("finished");
  });

  /** A node that failed writes nothing: the tour stops on the usual card, with a
   *  hint that says why the row is not there. */
  it("stops cleanly when a failed node left no output", () => {
    const fake = new FakeApp();
    fake.launch();
    fake.openRun();
    fake.selectNode();
    fake.finishNode("failed");

    let run = at(fake, "open-output");
    for (const clock of [0, 20_000]) run = observeTour(TOUR, run, fake.obs(), clock);

    expect(run.phase).toBe("failed");
    expect(run.failure).toMatchObject({ kind: "missing", stepId: "open-output" });
    expect(run.failure?.hint).toContain("writes no output");
  });
});

// ---- the end card ---------------------------------------------------------

describe("the recap", () => {
  it("names the Run the user actually created", () => {
    const fake = new FakeApp();
    fake.launch("release-check");

    const [first] = recapOf(TOUR, fake.app);
    expect(first.label).toBe("release-check");
    expect(first.text).toContain(TUTORIAL_REPO_PATH);
  });

  it("falls back to a neutral label when the list has not caught up", () => {
    const [first] = recapOf(TOUR, EMPTY_APP);
    expect(first.label).toBe("your Run");
  });

  /**
   * The card is the honest record of what the tour saw (#825). A reader who
   * skipped the wait must not be congratulated for a node that is still running,
   * and one whose node failed must not be told it finished on its own.
   */
  it("says where the node actually got to", () => {
    const fake = new FakeApp();
    fake.launch();
    fake.openRun();
    expect(recapOf(TOUR, fake.app)[1].text).toContain("still running");
    expect(recapOf(TOUR, fake.app)[1].text).toContain("guarded until a human releases it");

    fake.releaseCompletion();
    expect(recapOf(TOUR, fake.app)[1].text).toContain("released by you");

    fake.finishNode("failed");
    expect(recapOf(TOUR, fake.app)[1].text).toContain("ended");
    expect(recapOf(TOUR, fake.app)[1].text).not.toContain("finished on its own");

    fake.finishNode("completed");
    expect(recapOf(TOUR, fake.app)[1].text).toContain("finished on its own");
  });

  /** The tour no longer ends at Launch, so « Open the Run » would send the reader
   *  where they already are (#825). */
  it("ends on « done », with no button that goes nowhere", () => {
    expect(TOUR.outro?.title).toBeUndefined();
    expect(TOUR.outro?.primaryLabel).toBeUndefined();
    expect(TOUR.outro?.closing).toContain("every interactive node");
  });
});

// ---- the shape of the definition -----------------------------------------

describe("the definition holds the copy rules", () => {
  it("is the nine form steps plus the six reading ones", () => {
    expect(STEPS).toHaveLength(15);
    expect(STEPS.slice(9).map((s) => s.id)).toEqual([
      "find-run",
      "open-node",
      "talk-to-agent",
      "release-completion",
      "wait-for-node",
      "open-output",
    ]);
    expect(TOUR.minutes).toBeGreaterThanOrEqual(4);
  });

  it("gives every step an imperative title and at most two sentences", () => {
    const fake = new FakeApp();
    for (const step of STEPS) {
      expect(step.title, step.id).not.toMatch(/\.$/);
      const body = stepBody(step, fake.obs());
      const sentences = body.split(/(?<=[.!?])\s+/).filter(Boolean);
      expect(sentences.length, `"${step.id}" body: ${body}`).toBeLessThanOrEqual(2);
    }
  });

  /**
   * Every free input carries the exact block to paste — the prompt, the Run's
   * name, and the line to type into the agent's own session (#825) — and hands
   * the moment to the user instead of advancing the instant they stop typing.
   * For the terminal there is nothing to observe at all, which is the strongest
   * form of the same rule: `Next` is the only way on.
   */
  it("carries a block to paste for every free input, and waits for Next", () => {
    const free = ["name-run", "write-prompt", "talk-to-agent"];
    for (const step of STEPS) {
      expect(Boolean(step.copyBlock), step.id).toBe(free.includes(step.id));
      if (free.includes(step.id)) expect(step.confirm === true || !step.done, step.id).toBe(true);
    }
    expect(stepById("talk-to-agent").copyBlock).toBe(TUTORIAL_TERMINAL_LINE);
  });

  /**
   * The ticket says "Start"; the button says **Launch** (design Q1). The copy
   * names what is on screen — a tour that told someone to press a button that is
   * not there is worse than no tour. Same correction for "the session": the UI
   * calls it the terminal, inside the Run inspector (#825).
   */
  it("says Launch, because that is what the button says", () => {
    const fake = new FakeApp();
    const step = stepById("launch");
    expect(step.title).toContain("Launch");
    expect(stepBody(step, fake.obs())).toContain("Launch");
    const says = (s: TourStep, re: RegExp) =>
      re.test(stepBody(s, fake.obs())) || re.test(s.title);
    expect(STEPS.some((s) => says(s, /\bStart\b/))).toBe(false);
    expect(stepBody(stepById("talk-to-agent"), fake.obs())).toContain("terminal");
  });

  /** Every reading step is optional: a reader who already knows this half should
   *  not have to sit through it to earn the checkmark. */
  it("makes every reading step skippable, and only the wait unbounded", () => {
    for (const step of STEPS.slice(9)) expect(step.skippable, step.id).toBe(true);
    const unbounded = STEPS.filter((s) => s.targetTimeoutMs === null);
    expect(unbounded.map((s) => s.id)).toEqual(["wait-for-node"]);
  });

  it("gives every step a waitingFor, so a stop can name what it wanted", () => {
    for (const step of STEPS) expect(step.waitingFor.length, step.id).toBeGreaterThan(0);
  });

  it("declares both preparations, each with a failure headline", () => {
    const prepare = TOUR.intro!.prepare;
    expect(prepare.map((p) => p.id)).toEqual(["repo", "pipeline"]);
    for (const item of prepare) {
      expect(item.failureTitle.length).toBeGreaterThan(0);
      expect(item.pending.length).toBeGreaterThan(0);
      expect(item.ready.length).toBeGreaterThan(0);
      expect(typeof item.run).toBe("function");
    }
  });
});

// ---- the preparations -----------------------------------------------------

vi.mock("../../api", async () => {
  const actual = await vi.importActual<typeof import("../../api")>("../../api");
  return {
    ...actual,
    createRepo: vi.fn(async () => ({ path: "/tmp/pdo-tutorial", created: true })),
    fetchPipelines: vi.fn(async () => [] as unknown[]),
    createPipeline: vi.fn(async () => ({ id: TUTORIAL_RUN_PIPELINE_ID, scope: "instance", path: "p" })),
    savePipeline: vi.fn(async () => ({ ok: true })),
  };
});

describe("what the intro card prepares", () => {
  it("creates the training repository under /tmp with a README and notes.txt", async () => {
    const api = await import("../../api");
    await TOUR.intro!.prepare[0].run();

    expect(api.createRepo).toHaveBeenCalledWith("/tmp", "pdo-tutorial", [
      expect.objectContaining({ path: "README.md" }),
      expect.objectContaining({ path: "notes.txt" }),
    ]);
  });

  it("writes the tutorial pipeline when the Library does not have it", async () => {
    const api = await import("../../api");
    vi.mocked(api.fetchPipelines).mockResolvedValueOnce([]);

    await TOUR.intro!.prepare[1].run();

    expect(api.createPipeline).toHaveBeenCalledWith(TUTORIAL_RUN_PIPELINE_ID);
    const [, yaml, prompts] = vi.mocked(api.savePipeline).mock.calls.at(-1)!;
    // One interactive node, no harness pinned, one markdown output that says what
    // it must hold — and a prompt for the node.
    expect(yaml).toContain("interactive: true");
    expect(yaml).toContain("port_type: markdown");
    expect(yaml).toContain("Summary of what you changed");
    expect(yaml).not.toContain("pin_harness");
    expect(Object.keys(prompts)).toEqual(["assistant"]);
  });

  /**
   * The very first canvas a newcomer sees had two cards on top of each other
   * (#825, FP iteration 2): only `assistant` carried a `view`, so the canvas laid
   * Start and End out itself, in a column that ran straight under it.
   */
  it("gives every node of the tutorial pipeline a place of its own", async () => {
    const api = await import("../../api");
    vi.mocked(api.fetchPipelines).mockResolvedValueOnce([]);
    await TOUR.intro!.prepare[1].run();
    const [, yaml] = vi.mocked(api.savePipeline).mock.calls.at(-1)!;

    const views = [...yaml.matchAll(/view:\s*\{\s*x:\s*(-?\d+),\s*y:\s*(-?\d+)\s*\}/g)].map((m) => ({
      x: Number(m[1]),
      y: Number(m[2]),
    }));
    expect(views, "one per node: start, assistant, end").toHaveLength(3);
    for (let i = 0; i < views.length; i++) {
      for (let j = i + 1; j < views.length; j++) {
        const apart =
          Math.abs(views[i].x - views[j].x) >= CARD_WIDTH ||
          Math.abs(views[i].y - views[j].y) >= CARD_HEIGHT;
        expect(apart, `node ${i} sits on node ${j}`).toBe(true);
      }
    }
  });

  /**
   * Idempotence is the whole contract of a preparation: replaying the tour must be
   * free, and a user who edited the tutorial pipeline keeps their edit.
   */
  it("leaves an existing tutorial pipeline exactly as the user left it", async () => {
    const api = await import("../../api");
    vi.mocked(api.createPipeline).mockClear();
    vi.mocked(api.savePipeline).mockClear();
    vi.mocked(api.fetchPipelines).mockResolvedValueOnce([
      { id: TUTORIAL_RUN_PIPELINE_ID } as never,
    ]);

    await TOUR.intro!.prepare[1].run();

    expect(api.createPipeline).not.toHaveBeenCalled();
    expect(api.savePipeline).not.toHaveBeenCalled();
  });

  it("lets a refusal through, so the card can quote it", async () => {
    const api = await import("../../api");
    vi.mocked(api.createRepo).mockRejectedValueOnce(
      new Error("/tmp/pdo-tutorial exists and is not a git repository"),
    );

    await expect(TOUR.intro!.prepare[0].run()).rejects.toThrow(
      "exists and is not a git repository",
    );
  });
});
