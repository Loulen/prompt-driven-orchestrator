/**
 * How the *First pipeline* tour (#823) reads the pipeline the user is building —
 * the node, edge and condition lookups its steps aim and advance on, the
 * selectors they point at, and the two step factories the wiring half is built
 * from. Pure: every function takes the observed state and returns an answer.
 */

import type { EdgeDef } from "../../shared/generated/EdgeDef";
import type { TourAppState, TourObservation, TourStep } from "../tourDef";
import { carriedPorts, carries } from "../edgePorts";
import { splitFieldKey } from "../whenClause";
import type { EditorNodeDef, EditorPipelineDef } from "../../pipeline/model/pipeline";
import { withTourIdentity } from "../tourMemory";

/** The pipeline's name, before this browser's Identité de tour. */
export const TUTORIAL_PIPELINE_ID = "tutorial-implement-test";

/**
 * The name the tour asks the user to paste for the pipeline itself:
 * `tutorial-implement-test-k3x9`. Two people building it on one instance would
 * otherwise both ask for the same YAML file, and the second Create would be
 * refused.
 */
export function tutorialPipelineName(): string {
  return withTourIdentity(TUTORIAL_PIPELINE_ID);
}

// ---- reading the observed pipeline ---------------------------------------
// Every target below is resolved from state rather than hard-coded, because the
// nodes the tour points at do not exist until the user creates them — and they are
// born with generated ids.

/** The agent nodes, in creation order: [0] is the implementer, [1] the tester. */
export function agents(app: TourAppState): EditorNodeDef[] {
  return (app.pipeline?.nodes ?? []).filter((n) => n.type === "agent");
}

export function agent(app: TourAppState, i: number): EditorNodeDef | null {
  return agents(app)[i] ?? null;
}

export function startNode(app: TourAppState): EditorNodeDef | null {
  return (app.pipeline?.nodes ?? []).find((n) => n.type === "start") ?? null;
}

export function endNode(app: TourAppState): EditorNodeDef | null {
  return (app.pipeline?.nodes ?? []).find((n) => n.type === "end") ?? null;
}

/** xyflow stamps `data-id` with the node id — the one canvas target that is stable. */
export function nodeSel(node: EditorNodeDef | null): string[] {
  return node ? [`.react-flow__node[data-id="${cssEscape(node.id)}"]`] : [];
}

/**
 * Edges are keyed `e-<index>` on the canvas (`deriveEditEdges`), so an edge target
 * is its index in the document — which is also what `selection.edgeIndex` holds.
 *
 * `fromPort` matters once the source has more than one: the tester carries `out`
 * AND `image_list`, and only `out` holds the `verdict` the next step conditions
 * on. An edge that carries only another port is a dead end two steps later —
 * the When editor offers the fields of the ports it carries, and there is no
 * `verdict` in a screenshot (#825, FP iteration 2). So the step does not count it
 * as done, and sends the reader to the edge's Outputs section (#846).
 */
export function edgeIndex(
  pipeline: EditorPipelineDef | null,
  from: EditorNodeDef | null,
  to: EditorNodeDef | null,
  fromPort?: string,
): number {
  if (!pipeline || !from || !to) return -1;
  return pipeline.edges.findIndex(
    (e) =>
      e.source.node === from.id &&
      e.target.node === to.id &&
      (fromPort == null || carries(e.source, fromPort)),
  );
}

/** The tester's verdict port — the one every condition in this tour reads. */
export const OUT_PORT = "out";

export function edgeSel(
  app: TourAppState,
  from: EditorNodeDef | null,
  to: EditorNodeDef | null,
  fromPort?: string,
): string[] {
  const i = edgeIndex(app.pipeline, from, to, fromPort);
  return i < 0 ? [] : [`.react-flow__edge[data-id="e-${i}"]`];
}

/**
 * The tester → `to` edge the user drew without `out` on it, or -1 (#846). A
 * drawn edge carries the first declared output (ADR-0073), so this is the edge
 * whose outputs were changed in the panel, or a tester whose outputs were
 * reordered. The panel's Outputs section is where it is fixed — a redraw would
 * carry the same port again. Only counted while no tester → `to` edge carries
 * `out`: once one does, a stray one next to it is not the step's business.
 */
function strayEdge(app: TourAppState, to: EditorNodeDef | null): number {
  const from = agent(app, 1);
  if (edgeIndex(app.pipeline, from, to, OUT_PORT) >= 0) return -1;
  return edgeIndex(app.pipeline, from, to);
}

/** The tester → `to` edge carrying `out` is the selected one. */
export function edgeSelected(app: TourAppState, to: EditorNodeDef | null): boolean {
  const i = edgeIndex(app.pipeline, agent(app, 1), to, OUT_PORT);
  return i >= 0 && app.selection.kind === "edge" && app.selection.edgeIndex === i;
}

/** The edge panel's Outputs section — one checkbox per output of the source. */
const OUTPUTS_SECTION = '[data-testid="outputs-section"]';

/**
 * A step that draws an edge out of the tester and must end with `out` on it
 * (#846). It advances on the output the edge *carries*, not on how it was
 * drawn. When the edge carries another port, the same card re-aims: onto the
 * edge until it is selected, then onto the Outputs section, where ticking `out`
 * satisfies the step.
 */
export function carryOutEdgeStep(def: {
  id: string;
  title: string;
  to: (app: TourAppState) => EditorNodeDef | null;
  body: string;
  note: string;
  waitingFor: string;
}): TourStep {
  const stray = (o: TourObservation) => strayEdge(o.app, def.to(o.app));
  const straySelected = (o: TourObservation, i: number) =>
    o.app.selection.kind === "edge" && o.app.selection.edgeIndex === i;
  return {
    id: def.id,
    title: def.title,
    body: (o) => {
      const i = stray(o);
      if (i < 0) return def.body;
      const ports = carriedPorts(o.app.pipeline!.edges[i].source).join(" and ");
      return straySelected(o, i)
        ? `Tick out in the Outputs section on the right. This edge carries ${ports}, and only out holds the verdict the condition reads.`
        : `This edge carries ${ports}, not out. Click it, then tick out in the Outputs section of its inspector.`;
    },
    note: (o) => (stray(o) < 0 ? def.note : null),
    target: (o) => {
      const i = stray(o);
      if (i < 0) return [...nodeSel(agent(o.app, 1)), ...nodeSel(def.to(o.app))];
      return straySelected(o, i) ? [OUTPUTS_SECTION] : [`.react-flow__edge[data-id="e-${i}"]`];
    },
    waitingFor: def.waitingFor,
    done: (o) => edgeIndex(o.app.pipeline, agent(o.app, 1), def.to(o.app), OUT_PORT) >= 0,
  };
}

/** Attribute selectors here only ever carry ids the daemon already sanitised, but a
 *  quote in one would silently break the selector rather than fail loudly. */
export function cssEscape(value: string): string {
  return value.replace(/["\\]/g, "\\$&");
}

export function filled(o: TourObservation, selector: string): boolean {
  return (o.value(selector) ?? "").trim().length > 0;
}

export const NAME_INPUT = '[data-testid="node-name-input"]';
export const PROMPT_INPUT = '[data-testid="node-prompt-input"]';
export const ADD_MENU_NODE = '[data-testid="add-menu-node"]';
export const NEW_PIPELINE_DIALOG = '[data-testid="new-pipeline-dialog"]';
export const NEW_PIPELINE_NAME = '[data-testid="new-pipeline-name"]';
/** The dialog's own sentence when the daemon refused — quoted, never paraphrased. */
export const NEW_PIPELINE_ERROR = '[data-testid="new-pipeline-error"]';
/** An output card, addressed by its slot rather than its (editable) port name. */
export const outputSlot = (i: number) => `[data-output-index="${i}"]`;
/** The delete confirmation's box — the dialog itself, not its full-screen backdrop. */
export const DELETE_CONFIRM = '[data-testid="confirm-delete-modal"]';
/** Validate, next to Save, and the modal it opens (CONTEXT.md « Pipeline validé »). */
export const VALIDATE_BUTTON = '[data-testid="validate-button"]';
export const VALIDATE_MODAL = '[data-testid="validate-pipeline-modal"]';
/** The window Save… opens: a new version, kept apart, or replacing the last (ADR-0088). */
export const SAVE_MODAL = '[data-testid="save-version-modal"]';
/** The tab bar's indicator once the pipeline is offered to somebody. */
export const VALIDATED_FOR_ALL =
  '[data-testid="toolbar-validation-indicator"][data-validation="all"]';
export const VALIDATED_FOR_PROJECTS =
  '[data-testid="toolbar-validation-indicator"][data-validation="projects"]';

/** The dialog is gone and a pipeline is open: Create (or Enter) did its job. */
export function pipelineCreated(o: TourObservation): boolean {
  return !o.present(NEW_PIPELINE_DIALOG) && o.app.pipeline != null;
}

/**
 * The pipeline this tour built, while the Library still lists it: the name the
 * tour handed out to paste, or (for a reader who typed a name of their own) the
 * active tab, as long as it holds the tour's loop (tester → implementer and
 * tester → End). Deleting the pipeline closes its tab and activates whichever is
 * left; that one is not the tour's, even when the Library lists it. `null` once
 * the built pipeline is gone from the Library.
 */
export function builtPipelineId(o: TourObservation): string | null {
  const library = o.app.libraryPipelineIds;
  const mine = tutorialPipelineName();
  if (library.includes(mine)) return mine;
  const active = o.app.pipelineId;
  if (!active || !library.includes(active)) return null;
  const [implementer, tester] = [agent(o.app, 0), agent(o.app, 1)];
  const isTheLoop =
    edgeIndex(o.app.pipeline, tester, implementer) >= 0 && edgeIndex(o.app.pipeline, tester, endNode(o.app)) >= 0;
  return isTheLoop ? active : null;
}

export function selected(app: TourAppState, node: EditorNodeDef | null): boolean {
  return !!node && app.selection.kind === "node" && app.selection.id === node.id;
}

/**
 * Whether the edge's condition reads `field eq value` on the `out` port. The key
 * is resolved against the ports the edge carries, the way the daemon and the
 * When editor read it: bare `verdict` on an edge that carries only `out`,
 * `out.verdict` once it carries `out` and `image_list` — which is what every
 * edge fixed in the Outputs section carries (#846, FP iteration 1: the step read
 * the bare key only, and the correction path dead-ended here).
 */
function whenEquals(edge: EdgeDef | undefined, field: string, value: string): boolean {
  if (!edge?.when) return false;
  const ports = carriedPorts(edge.source);
  return Object.entries(edge.when).some(([key, predicate]) => {
    const at = splitFieldKey(key, ports);
    return (
      at.port === OUT_PORT &&
      at.field === field &&
      !!predicate &&
      typeof predicate === "object" &&
      (predicate as Record<string, unknown>).eq === value
    );
  });
}

/**
 * A step that writes `verdict eq <value>` on the tester → `to` edge. While the
 * edge holds some other condition, the body says what the step is waiting for,
 * so a reader whose clause is off by one field or value is not left staring at
 * a card that never moves (#846, FP iteration 1).
 *
 * It lights the When editor only while THIS edge is the selected one; otherwise
 * it lights the edge itself. Replayed with Previous (CONTEXT.md § « Rejeu
 * d'étape »), the step comes back with whatever edge the reader last selected
 * (the tester → End one, for « Only loop back on a failure »), and a generic
 * When editor would have pointed at the wrong edge's condition. Once the
 * condition holds, the body says so instead of « Not there yet » beside a live
 * Next.
 */
export function conditionStep(def: {
  id: string;
  title: string;
  to: (app: TourAppState) => EditorNodeDef | null;
  value: string;
  /** Why the condition matters — the second sentence of both bodies. */
  why: string;
}): TourStep {
  const edge = (app: TourAppState): EdgeDef | undefined => {
    const i = edgeIndex(app.pipeline, agent(app, 1), def.to(app), OUT_PORT);
    return i < 0 ? undefined : app.pipeline?.edges[i];
  };
  const done = (o: TourObservation) => whenEquals(edge(o.app), "verdict", def.value);
  return {
    id: def.id,
    title: def.title,
    body: (o) => {
      if (done(o)) return `Already set: the condition reads verdict eq ${def.value}. ${def.why}`;
      if (!edgeSelected(o.app, def.to(o.app)))
        return `Click the edge first, then add the condition verdict eq ${def.value} in its inspector. ${def.why}`;
      const when = edge(o.app)?.when;
      return !when || Object.keys(when).length === 0
        ? `Add the condition verdict eq ${def.value}. ${def.why}`
        : `Not there yet: the condition must read verdict eq ${def.value}, on the out output. ${def.why}`;
    },
    target: (o) =>
      edgeSelected(o.app, def.to(o.app))
        ? ['[data-testid="when-editor"]']
        : edgeSel(o.app, agent(o.app, 1), def.to(o.app), OUT_PORT),
    waitingFor: "the edge and its When editor",
    failureHint: "The edge was deleted before the condition was saved.",
    done,
  };
}
