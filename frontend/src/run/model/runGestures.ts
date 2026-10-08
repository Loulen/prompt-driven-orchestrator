/**
 * The gestures on a Run and on its nodes the caller may make (CORE-42), as
 * the view of a Run, the row of the Runs list and its bulk actions read them
 * to hide what the daemon would refuse.
 *
 * Asked with the question of authorization (`POST /authorize`): each gesture
 * is the Operation the daemon asks before running it — `run` × the command
 * (`pause_run`, `retry_all`, `cleanup_run`…) or `delete` for Forget, targeted
 * at the Run; `node` × the action (`stop`, `retry`, `mark_node_done`…),
 * targeted at `<run>/<node>`. Without the Projets: the daemon places them in
 * the Projet of the Run. Without extension every answer is « allow », and
 * nothing is hidden. The daemon still refuses what the UI shows.
 */

import type { Decision, OperationQuestion } from "../../shared/authorization/api";
import { isAllowed } from "../../shared/authorization/api";

/** A gesture on a Run the UI offers: the action the daemon asks about, or `delete` (Forget). */
export type RunGesture =
  | "pause_run"
  | "resume_run"
  | "reopen_run"
  | "retry_all"
  | "cleanup_run"
  | "rename_run"
  | "end_region"
  | "start_manager"
  | "stop_manager"
  | "delete";

/** A gesture on a node of a Run the UI offers: the action the daemon asks about. */
export type NodeGesture =
  | "stop"
  | "retry"
  | "start"
  | "mark_node_done"
  | "release_completion"
  | "kill_node"
  | "restart_node";

/** The gestures of the view of a Run on the Run itself (its toolbar, its loop regions, its manager). */
export const RUN_VIEW_GESTURES: readonly RunGesture[] = [
  "reopen_run",
  "retry_all",
  "end_region",
  "start_manager",
  "stop_manager",
];

/** The gestures of a row of the Runs list (its icons, its menu, the bulk bar). */
export const RUN_ROW_GESTURES: readonly RunGesture[] = [
  "pause_run",
  "resume_run",
  "retry_all",
  "cleanup_run",
  "rename_run",
  "delete",
];

/** Every gesture on a node the view of a Run offers. */
export const NODE_GESTURES: readonly NodeGesture[] = [
  "stop",
  "retry",
  "start",
  "mark_node_done",
  "release_completion",
  "kill_node",
  "restart_node",
];

/** The key of a decision: `<run>` × gesture, or `<run>/<node>` × gesture. */
function key(target: string, gesture: string): string {
  return `${target} ${gesture}`;
}

/**
 * The gestures refused, by Run. A Run nobody asked about, or a gesture not
 * asked about, is allowed: before the answer, and without it, everything
 * shows (the daemon decides).
 */
export interface GestureRights {
  /** `<target> <gesture>` of each refusal. */
  refused: ReadonlySet<string>;
}

export const NOTHING_REFUSED: GestureRights = { refused: new Set() };

/** What one Run asks about: the gestures on it, and on each of `nodeIds`. */
export interface GestureQuestion {
  runId: string;
  runGestures: readonly RunGesture[];
  nodeIds: readonly string[];
}

/** The Operations to ask about, in order, for `questions`. */
export function gestureQuestions(questions: readonly GestureQuestion[]): OperationQuestion[] {
  return questions.flatMap(({ runId, runGestures, nodeIds }) => [
    ...runGestures.map((gesture) => runOperation(runId, gesture)),
    ...nodeIds.flatMap((nodeId) =>
      NODE_GESTURES.map((gesture) => ({
        resource: "node",
        verb: { action: gesture },
        target: `${runId}/${nodeId}`,
      })),
    ),
  ]);
}

function runOperation(runId: string, gesture: RunGesture): OperationQuestion {
  return {
    resource: "run",
    verb: gesture === "delete" ? "delete" : { action: gesture },
    target: runId,
  };
}

/**
 * `rights` once `decisions` answered `questions` ({@link gestureQuestions}):
 * each gesture asked about takes its new answer, the others keep theirs.
 */
export function withDecisions(
  rights: GestureRights,
  questions: readonly GestureQuestion[],
  decisions: readonly Decision[],
): GestureRights {
  const refused = new Set(rights.refused);
  let i = 0;
  const answer = (k: string) => {
    if (isAllowed(decisions[i++])) refused.delete(k);
    else refused.add(k);
  };
  for (const { runId, runGestures, nodeIds } of questions) {
    for (const gesture of runGestures) answer(key(runId, gesture));
    for (const nodeId of nodeIds) {
      for (const gesture of NODE_GESTURES) answer(key(`${runId}/${nodeId}`, gesture));
    }
  }
  return { refused };
}

/** Whether the caller may make `gesture` on the Run `runId`. */
export function mayOnRun(rights: GestureRights, runId: string, gesture: RunGesture): boolean {
  return !rights.refused.has(key(runId, gesture));
}

/** Whether the caller may make `gesture` on the node `nodeId` of `runId`. */
export function mayOnNode(
  rights: GestureRights,
  runId: string,
  nodeId: string,
  gesture: NodeGesture,
): boolean {
  return !rights.refused.has(key(`${runId}/${nodeId}`, gesture));
}

/** How many Operations one `POST /authorize` may hold (the daemon's cap). */
export const MAX_OPERATIONS = 500;

/**
 * `questions` split so that no batch holds more than `max` Operations; a Run
 * whose own questions exceed it is a batch of its own, its nodes split.
 */
export function batches(questions: readonly GestureQuestion[], max = MAX_OPERATIONS): GestureQuestion[][] {
  const out: GestureQuestion[][] = [];
  let current: GestureQuestion[] = [];
  let size = 0;
  const flush = () => {
    if (current.length > 0) out.push(current);
    current = [];
    size = 0;
  };
  for (const question of questions) {
    for (const part of split(question, max)) {
      const n = part.runGestures.length + part.nodeIds.length * NODE_GESTURES.length;
      if (size + n > max) flush();
      current.push(part);
      size += n;
    }
  }
  flush();
  return out;
}

/** `question` in parts of at most `max` Operations each. */
function split(question: GestureQuestion, max: number): GestureQuestion[] {
  const n = NODE_GESTURES.length;
  if (question.runGestures.length + question.nodeIds.length * n <= max) return [question];
  const first = Math.max(0, Math.floor((max - question.runGestures.length) / n));
  const parts: GestureQuestion[] = [{ ...question, nodeIds: question.nodeIds.slice(0, first) }];
  const perPart = Math.floor(max / n);
  for (let i = first; i < question.nodeIds.length; i += perPart) {
    parts.push({ runId: question.runId, runGestures: [], nodeIds: question.nodeIds.slice(i, i + perPart) });
  }
  return parts;
}
