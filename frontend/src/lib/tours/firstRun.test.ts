/**
 * The *First run* tour definition (#824).
 *
 * Same two kinds of check as *First pipeline*. The **walk** plays the whole tour
 * against a simulated New Run form: the user does exactly the gesture each step
 * describes, and the machine must reach `finished` without stalling. A step whose
 * condition its own copy cannot satisfy is caught here rather than by a person
 * four minutes into a real tour.
 *
 * Then the cases the walk cannot express: the two-beat targets (trigger, then the
 * option inside the popup), the wrong pick that must NOT advance, the refused
 * Launch, and the recap that names the Run the user actually created.
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
  type TourAppState,
  type TourObservation,
} from "../tour";
import {
  FIRST_RUN_TOUR,
  TUTORIAL_PROMPT,
  TUTORIAL_REPO_PATH,
  TUTORIAL_RUN_PIPELINE_ID,
} from "./firstRun";

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
  app: TourAppState = { ...EMPTY_APP, runCount: 2, latestRun: { id: "r-old", name: "older" } };
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
      latestRun: { id: "r-new", name },
    };
  }

  /** A launch the daemon refused: the modal stays, with its error line showing. */
  refuseLaunch(reason: string) {
    this.say(LAUNCH_ERROR, reason);
  }
}

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

    if (needsConfirm(TOUR, run)) {
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
    expect(needsConfirm(TOUR, run)).toBe(true);
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
    run = { ...run, phase: "running" as const, index: STEPS.length - 1 };

    fake.refuseLaunch('harness "claude" not found on PATH');
    run = observeTour(TOUR, run, fake.obs(), 1_000);

    expect(run.phase).toBe("failed");
    expect(run.failure).toMatchObject({
      kind: "refused",
      stepId: "launch",
      stepNumber: STEPS.length,
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
    run = { ...run, phase: "running" as const, index: STEPS.length - 1 };

    fake.launch();
    fake.refuseLaunch("sandbox unavailable");
    run = observeTour(TOUR, run, fake.obs(), 1_000);

    expect(run.phase).toBe("failed");
    expect(run.failure?.reason).toBe("sandbox unavailable");
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

  it("ends on something that is starting, not on « done »", () => {
    expect(TOUR.outro?.title).toBe("Your Run is starting");
    expect(TOUR.outro?.primaryLabel).toBe("Open the Run");
  });

  /**
   * The training repository has never been seen by the harness, so the first
   * thing the « live session » shows is a trust prompt. The end card names it:
   * a security question nobody announced reads as the tour having broken
   * something.
   */
  it("warns that the harness will ask about the brand-new training repository", () => {
    expect(TOUR.outro?.closing).toContain("trust");
    expect(TOUR.outro?.closing).toContain(TUTORIAL_REPO_PATH);
  });
});

// ---- the shape of the definition -----------------------------------------

describe("the definition holds the copy rules", () => {
  it("is nine steps, about four minutes", () => {
    expect(STEPS).toHaveLength(9);
    expect(TOUR.minutes).toBe(4);
  });

  it("gives every step an imperative title and at most two sentences", () => {
    for (const step of STEPS) {
      expect(step.title, step.id).not.toMatch(/\.$/);
      const sentences = step.body.split(/(?<=[.!?])\s+/).filter(Boolean);
      expect(sentences.length, `"${step.id}" body: ${step.body}`).toBeLessThanOrEqual(2);
    }
  });

  it("carries a block to paste for every free input", () => {
    for (const step of STEPS) {
      const free = step.id === "name-run" || step.id === "write-prompt";
      expect(Boolean(step.copyBlock), step.id).toBe(free);
      // A free input also hands the moment to the user rather than auto-advancing
      // the instant they stop typing.
      if (free) expect(step.confirm, step.id).toBe(true);
    }
  });

  /**
   * The ticket says "Start"; the button says **Launch** (design Q1). The copy
   * names what is on screen — a tour that told someone to press a button that is
   * not there is worse than no tour.
   */
  it("says Launch, because that is what the button says", () => {
    const step = stepById("launch");
    expect(step.title).toContain("Launch");
    expect(step.body).toContain("Launch");
    expect(STEPS.some((s) => /\bStart\b/.test(s.body) || /\bStart\b/.test(s.title))).toBe(false);
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
