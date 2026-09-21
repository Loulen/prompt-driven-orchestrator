/**
 * The *First pipeline* tour (#823, spec #821) — pure data.
 *
 * Order and content come from the ticket: create a pipeline, build an
 * **implementer** and a **tester**, give the tester a `verdict` enum and an
 * annotated screenshot, then wire the loop (`verdict eq fail` back to the
 * implementer, `verdict eq pass` to End), save, and decide whether to keep it.
 *
 * Copy rules the implementer of a new step must keep (design round 1):
 * - Title is an imperative, body is two sentences at most.
 * - Every free input carries the exact block to paste.
 * - **"Interactive" never "opens a terminal".** Every node's terminal is open;
 *   Interactive means the node cannot complete until a human releases it, and that
 *   it will wait on your answers along the way.
 */

import type { NodeDef, PipelineDef } from "../../types";
import type { TourAppState, TourDef, TourObservation, TourStep } from "../tour";

/** The name the tour asks the user to paste for the pipeline itself. */
export const TUTORIAL_PIPELINE_ID = "tutorial-implement-test";

const IMPLEMENTER_PROMPT = `Implement what the user asked for, then run the project's tests.
Write the test results and a mermaid diagram of your change into your output file.`;

const TESTER_PROMPT = `Review the implementer's change and exercise the feature yourself.
Put verdict: pass in your output's frontmatter when it holds up, verdict: fail otherwise, and attach an annotated screenshot.`;

const IMPLEMENTER_OUT = "Test results and a mermaid diagram of the change";
const TESTER_OUT = "A short verdict, and what failed if anything did";
const TESTER_SHOT = "Annotated screenshot of the feature";

// ---- reading the observed pipeline ---------------------------------------
// Every target below is resolved from state rather than hard-coded, because the
// nodes the tour points at do not exist until the user creates them — and they are
// born with generated ids.

/** The agent nodes, in creation order: [0] is the implementer, [1] the tester. */
function agents(app: TourAppState): NodeDef[] {
  return (app.pipeline?.nodes ?? []).filter((n) => n.type === "agent");
}

function agent(app: TourAppState, i: number): NodeDef | null {
  return agents(app)[i] ?? null;
}

function endNode(app: TourAppState): NodeDef | null {
  return (app.pipeline?.nodes ?? []).find((n) => n.type === "end") ?? null;
}

/** xyflow stamps `data-id` with the node id — the one canvas target that is stable. */
function nodeSel(node: NodeDef | null): string[] {
  return node ? [`.react-flow__node[data-id="${cssEscape(node.id)}"]`] : [];
}

/**
 * Edges are keyed `e-<index>` on the canvas (`deriveEditEdges`), so an edge target
 * is its index in the document — which is also what `selection.edgeIndex` holds.
 *
 * `fromPort` matters once the source has more than one: the tester carries `out`
 * AND `image_list`, and only `out` holds the `verdict` the next step conditions
 * on. An edge drawn from the wrong handle is a dead end two steps later — the
 * When editor offers the fields of the port it came from, and there is no
 * `verdict` in a screenshot (#825, FP iteration 2). So the step does not count it
 * as done, and goes on asking for the one it named.
 */
function edgeIndex(
  pipeline: PipelineDef | null,
  from: NodeDef | null,
  to: NodeDef | null,
  fromPort?: string,
): number {
  if (!pipeline || !from || !to) return -1;
  return pipeline.edges.findIndex(
    (e) =>
      e.source.node === from.id &&
      e.target.node === to.id &&
      (fromPort == null || e.source.port === fromPort),
  );
}

/** The tester's verdict port — the one every condition in this tour reads. */
const OUT_PORT = "out";

function edgeSel(
  app: TourAppState,
  from: NodeDef | null,
  to: NodeDef | null,
  fromPort?: string,
): string[] {
  const i = edgeIndex(app.pipeline, from, to, fromPort);
  return i < 0 ? [] : [`.react-flow__edge[data-id="e-${i}"]`];
}

/** Attribute selectors here only ever carry ids the daemon already sanitised, but a
 *  quote in one would silently break the selector rather than fail loudly. */
function cssEscape(value: string): string {
  return value.replace(/["\\]/g, "\\$&");
}

function filled(o: TourObservation, selector: string): boolean {
  return (o.value(selector) ?? "").trim().length > 0;
}

const NAME_INPUT = '[data-testid="node-name-input"]';
const PROMPT_INPUT = '[data-testid="node-prompt-input"]';
const ADD_MENU_NODE = '[data-testid="add-menu-node"]';
const NEW_PIPELINE_DIALOG = '[data-testid="new-pipeline-dialog"]';
/** The dialog's own sentence when the daemon refused — quoted, never paraphrased. */
const NEW_PIPELINE_ERROR = '[data-testid="new-pipeline-error"]';
/** An output card, addressed by its slot rather than its (editable) port name. */
const outputSlot = (i: number) => `[data-output-index="${i}"]`;
/** The delete confirmation's box — the dialog itself, not its full-screen backdrop. */
const DELETE_CONFIRM = '[data-testid="confirm-delete-modal"]';

function selected(app: TourAppState, node: NodeDef | null): boolean {
  return !!node && app.selection.kind === "node" && app.selection.id === node.id;
}

function whenEquals(when: Record<string, unknown> | null | undefined, field: string, value: string): boolean {
  const predicate = when?.[field];
  if (!predicate || typeof predicate !== "object") return false;
  return (predicate as Record<string, unknown>).eq === value;
}

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
    body: "Paste the name below into the field. The name becomes the YAML file in your Library.",
    target: () => [NEW_PIPELINE_DIALOG],
    waitingFor: "the New Pipeline dialog",
    failureHint: "The dialog was closed before the name was entered.",
    copyBlock: TUTORIAL_PIPELINE_ID,
    confirm: true,
    done: (o) => filled(o, '[data-testid="new-pipeline-name"]'),
  },
  {
    id: "create-pipeline",
    title: "Create it",
    body: "Click Create. PDO writes the file and opens it in the editor, with a Start and an End marker already in place.",
    target: () => ['[data-testid="new-pipeline-create"]'],
    waitingFor: "the Create button",
    // A reader who kept the pipeline at the end of a previous run of this tour
    // meets a name that is already taken, and the daemon refuses. Nothing here
    // can advance, so the tour says so instead of waiting on a button that will
    // go on doing nothing (#825, FP iteration 2).
    refused: (o) => o.text(NEW_PIPELINE_ERROR),
    refusalTitle: "The pipeline could not be created",
    refusalHint: `You may already have a ${TUTORIAL_PIPELINE_ID} from a previous run of this tour — open it from the Pipelines list, or delete it and start the tour again.`,
    done: (o) => !o.present(NEW_PIPELINE_DIALOG) && o.app.pipeline != null,
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
    body: "PDO already called it implementer, which is the name this tour uses. This field is where you would change it.",
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
  },
  {
    id: "select-tester",
    title: "Select the second node",
    body: "Click the new card so the inspector follows it. Both cards look alike until you name them.",
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
    target: () => [`${outputSlot(0)} [data-testid="output-schema-editor"]`],
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
    id: "edge-implementer-tester",
    title: "Connect implementer to tester",
    body: "Drag from the handle under implementer onto tester. The edge is the hand-off: when the first finishes, the second starts.",
    target: (o) => [...nodeSel(agent(o.app, 0)), ...nodeSel(agent(o.app, 1))],
    waitingFor: "the two agent cards",
    done: (o) => edgeIndex(o.app.pipeline, agent(o.app, 0), agent(o.app, 1)) >= 0,
  },
  {
    id: "edge-tester-implementer",
    title: "Draw the way back",
    body: "Now drag from the tester's out handle onto implementer. That second edge is what makes this a loop rather than a line.",
    // The tester grew a second output two steps ago, and the two handles look
    // alike. An edge out of `image_list` carries a screenshot, not a verdict, so
    // the condition the next step writes would have no field to read (#825).
    note: "The lower handle is image_list — an edge from it carries the screenshot, and no verdict to route on.",
    target: (o) => [...nodeSel(agent(o.app, 1)), ...nodeSel(agent(o.app, 0))],
    waitingFor: "the two agent cards",
    done: (o) => edgeIndex(o.app.pipeline, agent(o.app, 1), agent(o.app, 0), OUT_PORT) >= 0,
  },
  {
    id: "select-loop-edge",
    title: "Select the edge back to implementer",
    body: "Click the edge you just drew. Its inspector opens on the right, where a condition is authored.",
    target: (o) => edgeSel(o.app, agent(o.app, 1), agent(o.app, 0), OUT_PORT),
    waitingFor: "the tester → implementer edge",
    done: (o) =>
      o.app.selection.kind === "edge" &&
      o.app.selection.edgeIndex ===
        edgeIndex(o.app.pipeline, agent(o.app, 1), agent(o.app, 0), OUT_PORT),
  },
  {
    id: "loop-condition",
    title: "Only loop back on a failure",
    body: "Add the condition verdict eq fail. The edge now fires only when the tester's frontmatter says the change did not hold up.",
    target: () => ['[data-testid="when-editor"]'],
    waitingFor: "the When editor of the selected edge",
    failureHint: "The edge was deselected before the condition was saved.",
    done: (o) => {
      const i = edgeIndex(o.app.pipeline, agent(o.app, 1), agent(o.app, 0), OUT_PORT);
      return i >= 0 && whenEquals(o.app.pipeline?.edges[i]?.when, "verdict", "fail");
    },
  },
  {
    id: "edge-tester-end",
    title: "Connect tester to End",
    body: "Drag from the tester's out handle onto the End marker. End is where a Run stops, and it needs a route in.",
    // Same pair of look-alike handles as the loop edge, same consequence: the
    // `verdict eq pass` of the next step can only be written on an edge that
    // carries the verdict (#825).
    note: "Again out, not image_list: the condition below it reads the verdict in out's frontmatter.",
    target: (o) => [...nodeSel(agent(o.app, 1)), ...nodeSel(endNode(o.app))],
    waitingFor: "the tester card and the End marker",
    done: (o) => edgeIndex(o.app.pipeline, agent(o.app, 1), endNode(o.app), OUT_PORT) >= 0,
  },
  {
    id: "select-end-edge",
    title: "Select the edge to End",
    body: "Click that last edge. Same inspector, one more condition to write.",
    target: (o) => edgeSel(o.app, agent(o.app, 1), endNode(o.app), OUT_PORT),
    waitingFor: "the tester → End edge",
    done: (o) =>
      o.app.selection.kind === "edge" &&
      o.app.selection.edgeIndex ===
        edgeIndex(o.app.pipeline, agent(o.app, 1), endNode(o.app), OUT_PORT),
  },
  {
    id: "end-condition",
    title: "Finish only on a pass",
    body: "Add the condition verdict eq pass. Both routes out of the tester are now explicit: pass ends the Run, fail sends it round again.",
    target: () => ['[data-testid="when-editor"]'],
    waitingFor: "the When editor of the selected edge",
    failureHint: "The edge was deselected before the condition was saved.",
    done: (o) => {
      const i = edgeIndex(o.app.pipeline, agent(o.app, 1), endNode(o.app), OUT_PORT);
      return i >= 0 && whenEquals(o.app.pipeline?.edges[i]?.when, "verdict", "pass");
    },
  },
  {
    id: "save",
    title: "Save the pipeline",
    body: "Click Save. Until you do, all of this lives only in this browser tab.",
    target: () => ['[data-testid="save-button"]'],
    waitingFor: "the Save button",
    // Not skippable: everything after this reads the saved file.
    done: (o) => !o.app.dirty && !!o.app.pipelineId && o.app.libraryPipelineIds.includes(o.app.pipelineId),
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
      return o.app.pipelineId
        ? [`[data-testid="library-row-${cssEscape(o.app.pipelineId)}"]`]
        : [];
    },
    waitingFor: "the pipeline's row in the Library",
    skippable: true,
    done: (o) => !!o.app.pipelineId && !o.app.libraryPipelineIds.includes(o.app.pipelineId),
  },
];

export const FIRST_PIPELINE_TOUR: TourDef = {
  id: "first-pipeline",
  title: "First pipeline",
  blurb: "Two agents in a loop: an implementer and a tester that hands back a verdict.",
  minutes: 8,
  steps: STEPS,
  recapIntro: `You built ${TUTORIAL_PIPELINE_ID} and saved it to your Library. Here is what it does:`,
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
  ],
  // #825 — what the card of a Full tour's *previous* leg says when that leg was
  // refused. *First run* is the one that can be stopped by the machine (no
  // harness on PATH, no sandbox); this tour never spawns anything, so a refusal
  // there is no reason to abandon the chain.
  chainNote: "This one does not need a running agent: it only builds a pipeline.",
};
