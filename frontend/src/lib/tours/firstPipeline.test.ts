/**
 * The *First pipeline* tour definition (#823).
 *
 * Two kinds of check. The **walk** is the important one: it plays the whole tour
 * against a simulated app — the user performs the gesture each step asks for, and
 * the machine must reach `finished` without ever stalling or failing. A step whose
 * condition cannot be satisfied by the gesture it describes is caught here rather
 * than by a human halfway through a real tour.
 *
 * The **shape** checks hold the copy rules the design round fixed: an imperative
 * title, two sentences at most, a block for every free input, and — the correction
 * the user made in person — "Interactive" NEVER claims to open a terminal.
 */
import { describe, it, expect } from "vitest";
import {
  canAdvance,
  confirmStep,
  currentStep,
  needsConfirm,
  observeTour,
  skipStep,
  startTour,
  stepBody,
  type TourAppState,
  type TourObservation,
  type TourStep,
} from "../tour";
import type { EdgeDef, NodeDef, PipelineDef } from "../../types";
import { FIRST_PIPELINE_TOUR, TUTORIAL_PIPELINE_ID } from "./firstPipeline";

const STEPS = FIRST_PIPELINE_TOUR.steps;

// ---- the simulator --------------------------------------------------------

/**
 * A stand-in for the app: a pipeline document, a DOM the tour can see, and the
 * field values the user is typing. The gestures below move it exactly the way the
 * real UI would — a Create button opens the editor, adding a node drops a card on
 * the canvas, and so on.
 */
class FakeApp {
  app: TourAppState = {
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
  /** Frozen at construction: what "this tour caused it" is measured against. */
  readonly baseline: TourAppState = { ...this.app };
  private dom = new Set<string>(['[data-testid="left-tab-library"]']);
  private values: Record<string, string> = {};

  obs(): TourObservation {
    return {
      app: this.app,
      baseline: this.baseline,
      present: (selector) => this.dom.has(selector) || this.canvas(selector),
      value: (selector) => this.values[selector] ?? null,
      text: (selector) => this.values[selector] ?? null,
    };
  }

  /** Canvas targets are derived, not registered: xyflow paints one element per
   *  node and per edge, so "is it there?" is "is it in the document?". */
  private canvas(selector: string): boolean {
    const node = /^\.react-flow__node\[data-id="(.+)"\]$/.exec(selector);
    if (node) return !!this.app.pipeline?.nodes.some((n) => n.id === node[1]);
    const edge = /^\.react-flow__edge\[data-id="e-(\d+)"\]$/.exec(selector);
    if (edge) return (this.app.pipeline?.edges.length ?? 0) > Number(edge[1]);
    return false;
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

  agents(): NodeDef[] {
    return (this.app.pipeline?.nodes ?? []).filter((n) => n.type === "agent");
  }

  createPipeline() {
    this.app = {
      ...this.app,
      pipelineId: TUTORIAL_PIPELINE_ID,
      pipeline: {
        name: TUTORIAL_PIPELINE_ID,
        variables: {},
        // The daemon's scaffold: a start and an end marker, no edges.
        nodes: [
          { id: "start", name: "Start", type: "start", inputs: [], outputs: [], interactive: false },
          { id: "end", name: "End", type: "end", inputs: [], outputs: [], interactive: false },
        ],
        edges: [],
      },
      dirty: false,
    };
  }

  addAgent(id: string) {
    this.patch((p) => {
      p.nodes = [
        ...p.nodes,
        {
          id,
          // The canvas names every new agent `implementer` (`DEFAULT_NODE_NAMES`).
          name: "implementer",
          type: "agent",
          interactive: false,
          isolated_worktree: true,
          inputs: [{ name: "in", repeated: false, side: "left" }],
          outputs: [{ name: "out", repeated: false, side: "right" }],
        },
      ];
    });
  }

  select(id: string) {
    this.app = { ...this.app, selection: { kind: "node", id, edgeIndex: null } };
  }

  selectEdge(index: number) {
    this.app = { ...this.app, selection: { kind: "edge", id: null, edgeIndex: index } };
  }

  updateAgent(index: number, updates: Partial<NodeDef>) {
    const target = this.agents()[index];
    this.patch((p) => {
      p.nodes = p.nodes.map((n) => (n.id === target.id ? { ...n, ...updates } : n));
    });
  }

  addEdge(edge: EdgeDef) {
    this.patch((p) => {
      p.edges = [...p.edges, edge];
    });
  }

  setEdgeWhen(index: number, when: Record<string, unknown>) {
    this.patch((p) => {
      p.edges = p.edges.map((e, i) => (i === index ? { ...e, when } : e));
    });
  }

  save() {
    this.app = {
      ...this.app,
      dirty: false,
      libraryPipelineIds: [...this.app.libraryPipelineIds, TUTORIAL_PIPELINE_ID],
    };
  }

  private patch(fn: (p: PipelineDef) => void) {
    const pipeline = { ...this.app.pipeline! };
    fn(pipeline);
    this.app = { ...this.app, pipeline, dirty: true };
  }
}

const NAME_INPUT = '[data-testid="node-name-input"]';
const PROMPT_INPUT = '[data-testid="node-prompt-input"]';
const ADD_MENU = '[data-testid="add-menu-node"]';

/**
 * What the user does at each step, keyed by step id. The keys are checked against
 * the definition below, so a new step without a gesture fails loudly instead of
 * being silently walked past.
 */
function gestures(fake: FakeApp): Record<string, () => void> {
  const outSlot = '[data-output-index="0"]';
  const shotSlot = '[data-output-index="1"]';
  const inspector = () =>
    fake.show(NAME_INPUT, PROMPT_INPUT, outSlot, '[data-testid="node-interactive-toggle"]',
      '[data-testid="workspace-shared"]', '[data-testid="workspace-isolated"]',
      '[data-testid="add-output-port"]', `${outSlot} [data-testid="output-schema-editor"]`);

  return {
    "pipelines-tab": () => fake.show('[data-testid="new-pipeline-button"]'),
    "new-pipeline": () =>
      fake.show('[data-testid="new-pipeline-dialog"]', '[data-testid="new-pipeline-create"]'),
    "name-pipeline": () => fake.type('[data-testid="new-pipeline-name"]', TUTORIAL_PIPELINE_ID),
    "create-pipeline": () => {
      fake.hide('[data-testid="new-pipeline-dialog"]', '[data-testid="new-pipeline-create"]');
      fake.createPipeline();
      fake.show('[data-testid="toolbar-add"]', '[data-testid="save-button"]');
    },
    "open-add-menu": () => fake.show(ADD_MENU),
    "choose-node": () => {
      fake.hide(ADD_MENU);
      fake.addAgent("n1");
    },
    "select-implementer": () => {
      fake.select("n1");
      inspector();
      fake.type(NAME_INPUT, "implementer");
      fake.type(PROMPT_INPUT, "");
    },
    "name-implementer": () => fake.type(NAME_INPUT, "implementer"),
    "implementer-interactive": () => fake.updateAgent(0, { interactive: true }),
    "implementer-worktree": () => fake.updateAgent(0, { isolated_worktree: false }),
    "implementer-prompt": () => fake.type(PROMPT_INPUT, "Implement what the user asked for."),
    "implementer-out": () =>
      fake.updateAgent(0, {
        outputs: [{ name: "out", repeated: false, instructions: "Test results and a diagram" }],
      }),
    "open-add-menu-2": () => fake.show(ADD_MENU),
    "choose-node-2": () => {
      fake.hide(ADD_MENU);
      fake.addAgent("n2");
    },
    "select-tester": () => {
      fake.select("n2");
      fake.type(NAME_INPUT, "implementer");
      fake.type(PROMPT_INPUT, "");
    },
    "name-tester": () => fake.type(NAME_INPUT, "tester"),
    "tester-interactive": () => fake.updateAgent(1, { interactive: true }),
    // Nothing to do: a new agent is already isolated. The step is an acknowledge.
    "tester-worktree": () => {},
    "tester-prompt": () => fake.type(PROMPT_INPUT, "Review the change and test it."),
    "tester-out": () =>
      fake.updateAgent(1, {
        outputs: [{ name: "out", repeated: false, instructions: "A short verdict" }],
      }),
    "tester-verdict": () =>
      fake.updateAgent(1, {
        outputs: [
          {
            name: "out",
            repeated: false,
            instructions: "A short verdict",
            frontmatter: { verdict: { type: "enum", allowed: ["pass", "fail"] } },
          },
        ],
      }),
    "add-image-port": () => {
      // `handleAddOutput` names it `out-2` (the first free `out-N`).
      fake.updateAgent(1, {
        outputs: [...fake.agents()[1].outputs, { name: "out-2", repeated: false, side: "right" }],
      });
      fake.show(shotSlot);
    },
    "name-image-port": () =>
      fake.updateAgent(1, {
        outputs: [
          fake.agents()[1].outputs[0],
          { name: "image_list", repeated: false, port_type: "image_list" },
        ],
      }),
    "image-port-expected": () =>
      fake.updateAgent(1, {
        outputs: [
          fake.agents()[1].outputs[0],
          {
            name: "image_list",
            repeated: false,
            port_type: "image_list",
            instructions: "Annotated screenshot of the feature",
          },
        ],
      }),
    "edge-implementer-tester": () =>
      fake.addEdge({ source: { node: "n1", port: "out" }, target: { node: "n2", port: "in" } }),
    "edge-tester-implementer": () =>
      fake.addEdge({ source: { node: "n2", port: "out" }, target: { node: "n1", port: "in" } }),
    "select-loop-edge": () => {
      fake.selectEdge(1);
      fake.show('[data-testid="when-editor"]');
    },
    "loop-condition": () => fake.setEdgeWhen(1, { verdict: { eq: "fail" } }),
    "edge-tester-end": () =>
      fake.addEdge({ source: { node: "n2", port: "out" }, target: { node: "end", port: "result" } }),
    "select-end-edge": () => fake.selectEdge(2),
    "end-condition": () => fake.setEdgeWhen(2, { verdict: { eq: "pass" } }),
    save: () => {
      fake.save();
      fake.show(`[data-testid="library-row-${TUTORIAL_PIPELINE_ID}"]`);
    },
    // Keep or delete is deliberately unconstrained: keeping it is a Skip.
    "keep-or-delete": () => {},
  };
}

describe("walking the whole tour", () => {
  it("reaches the end when the user performs the gesture each step asks for", () => {
    const fake = new FakeApp();
    const script = gestures(fake);
    let run = startTour(FIRST_PIPELINE_TOUR, fake.obs());
    let clock = 0;
    const visited: string[] = [];

    // One iteration per step, plus slack; an infinite loop would be the bug.
    for (let guard = 0; guard < STEPS.length * 3 && run.phase === "running"; guard++) {
      const step = currentStep(FIRST_PIPELINE_TOUR, run)!;
      if (visited.at(-1) !== step.id) visited.push(step.id);

      const gesture = script[step.id];
      expect(gesture, `no gesture scripted for step "${step.id}"`).toBeDefined();
      gesture();

      clock += 200;
      run = observeTour(FIRST_PIPELINE_TOUR, run, fake.obs(), clock);
      if (run.phase !== "running" || currentStep(FIRST_PIPELINE_TOUR, run)?.id !== step.id) continue;

      // Still here: the step wants an explicit gesture from the popover.
      if (needsConfirm(FIRST_PIPELINE_TOUR, run, fake.obs())) {
        expect(canAdvance(FIRST_PIPELINE_TOUR, run, fake.obs()), `Next stuck on "${step.id}"`).toBe(
          true,
        );
        run = confirmStep(FIRST_PIPELINE_TOUR, run, fake.obs());
      } else if (step.skippable) {
        run = skipStep(FIRST_PIPELINE_TOUR, run, fake.obs());
      }
      expect(currentStep(FIRST_PIPELINE_TOUR, run)?.id, `stalled on "${step.id}"`).not.toBe(step.id);
    }

    expect(run.phase).toBe("finished");
    expect(visited).toHaveLength(STEPS.length);
    expect(visited).toEqual(STEPS.map((s) => s.id));
  });

  it("produces the pipeline the ticket describes", () => {
    // The same script, read as a result rather than as a path: two interactive
    // agents, opposite isolation, a verdict enum, an image_list port, and three
    // edges of which two are conditional.
    const fake = new FakeApp();
    const script = gestures(fake);
    for (const step of STEPS) script[step.id]();

    const [implementer, tester] = fake.agents();
    expect(implementer.interactive && tester.interactive).toBe(true);
    expect(implementer.isolated_worktree).toBe(false);
    expect(tester.isolated_worktree).toBe(true);
    expect(tester.outputs[0].frontmatter?.verdict).toEqual({
      type: "enum",
      allowed: ["pass", "fail"],
    });
    expect(tester.outputs[1]).toMatchObject({ name: "image_list", port_type: "image_list" });
    expect(fake.app.pipeline?.edges.map((e) => e.when)).toEqual([
      undefined,
      { verdict: { eq: "fail" } },
      { verdict: { eq: "pass" } },
    ]);
  });
});

/** Every *First pipeline* step declares a plain string body; resolving it needs
 *  an observation all the same, so the shape checks below borrow a fresh one. */
const SHAPE_OBS = new FakeApp().obs();
const bodyOf = (step: TourStep) => stepBody(step, SHAPE_OBS);

describe("the shape of every step", () => {
  it("has a unique id", () => {
    expect(new Set(STEPS.map((s) => s.id)).size).toBe(STEPS.length);
  });

  it("says one thing, in two sentences at most", () => {
    for (const step of STEPS) {
      expect(step.title.length, step.id).toBeLessThanOrEqual(64);
      expect(step.title.endsWith("."), step.id).toBe(false);
      const body = bodyOf(step);
      const sentences = body.split(/(?<=[.!?])\s+/).filter(Boolean);
      expect(sentences.length, `${step.id}: ${body}`).toBeLessThanOrEqual(3);
      expect(body.trim(), step.id).not.toBe("");
    }
  });

  it("gives a block to paste for every free input", () => {
    for (const step of STEPS.filter((s) => s.confirm)) {
      expect(step.copyBlock, `${step.id} asks for typing without a block`).toBeTruthy();
    }
  });

  it("names what it is waiting for, so a stop can say it", () => {
    for (const step of STEPS) expect(step.waitingFor.trim(), step.id).not.toBe("");
  });

  it("makes Save non-skippable and Keep-or-delete skippable", () => {
    expect(STEPS.find((s) => s.id === "save")?.skippable).toBeFalsy();
    expect(STEPS.find((s) => s.id === "keep-or-delete")?.skippable).toBe(true);
  });

  it("never claims Interactive opens a terminal", () => {
    // The correction the user made on the design, round 1. Every node has a
    // terminal; Interactive is about completion and about waiting on answers.
    for (const step of STEPS.filter((s) => s.id.includes("interactive"))) {
      expect(bodyOf(step).toLowerCase(), step.id).not.toMatch(/opens? a terminal/);
      expect(bodyOf(step).toLowerCase(), step.id).toContain("complete");
    }
  });
});
