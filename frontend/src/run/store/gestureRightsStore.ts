import { useCallback, useEffect } from "react";
import { create } from "zustand";
import { askAuthorization } from "../../shared/authorization/api";
import type { RunState } from "../../shared/generated/RunState";
import {
  batches,
  gestureQuestions,
  whyNotOnNode,
  whyNotOnRun,
  NOTHING_REFUSED,
  RUN_ROW_GESTURES,
  RUN_VIEW_GESTURES,
  withDecisions,
  type GestureQuestion,
  type GestureRights,
  type NodeGesture,
  type RunGesture,
} from "../model/runGestures";

/**
 * The gestures the caller may make on the Runs on screen (CORE-42), shared by
 * the view of a Run, the rows of the Runs list and the bulk bar.
 *
 * - The view of a Run asks once when it opens (every gesture on the Run and
 *   on each of its nodes, in one question), not once per button.
 * - The list asks about the gestures of a row for each Run it shows, once
 *   per Run.
 * - Everything asked is asked again when the rights or the Projets may have
 *   changed ({@link refreshGestureRights}): a Projet changed, an extension
 *   signalled that the Triggers changed, the window came back.
 *
 * A failed question keeps the last answer, and an answer older than the
 * last question about the same Run is dropped; before any answer nothing is
 * greyed. The daemon still refuses what the UI shows.
 */
interface GestureRightsState {
  rights: GestureRights;
  /** What the open view of a Run asked last. */
  viewed: GestureQuestion | null;
  /** The Runs of the list. */
  listed: ReadonlySet<string>;
  /** Ask what the view of the Run `runId`, with its nodes `nodeIds`, offers. */
  askView: (runId: string, nodeIds: readonly string[]) => Promise<void>;
  /** The Runs of the list are `runIds`: ask the gestures of a row for those not asked yet. */
  askRows: (runIds: readonly string[]) => Promise<void>;
  /** Ask again everything asked. */
  refresh: () => Promise<void>;
}

/** Ask `questions` (split under the daemon's cap); the decisions, in order. */
async function ask(questions: GestureQuestion[]) {
  const parts = batches(questions);
  const decisions = await Promise.all(parts.map((part) => askAuthorization(gestureQuestions(part))));
  return { asked: parts.flat(), decisions: decisions.flat() };
}

function rowQuestion(runId: string): GestureQuestion {
  return { runId, runGestures: RUN_ROW_GESTURES, nodeIds: [] };
}

/** What a question is, for « was it asked again since? »: the view's or a row's, of one Run. */
function slot(question: GestureQuestion): string {
  return `${question.nodeIds.length > 0 || question.runGestures === RUN_VIEW_GESTURES ? "view" : "row"} ${question.runId}`;
}

export const useGestureRightsStore = create<GestureRightsState>((set, get) => {
  let generation = 0;
  /** The generation of the last question of each slot. */
  const latest = new Map<string, number>();
  const answer = async (questions: GestureQuestion[]) => {
    if (questions.length === 0) return;
    const asked = ++generation;
    for (const question of questions) latest.set(slot(question), asked);
    try {
      const answered = await ask(questions);
      // Keep only what no later question asked again.
      const current: GestureQuestion[] = [];
      const decisions: typeof answered.decisions = [];
      let i = 0;
      for (const question of answered.asked) {
        const n = gestureQuestions([question]).length;
        if (latest.get(slot(question)) === asked) {
          current.push(question);
          decisions.push(...answered.decisions.slice(i, i + n));
        }
        i += n;
      }
      set((s) => ({ rights: withDecisions(s.rights, current, decisions) }));
    } catch {
      // keep the last answer: the daemon decides at the gesture
    }
  };
  return {
    rights: NOTHING_REFUSED,
    viewed: null,
    listed: new Set(),
    askView: async (runId, nodeIds) => {
      const viewed = { runId, runGestures: RUN_VIEW_GESTURES, nodeIds: [...nodeIds] };
      set({ viewed });
      await answer([viewed]);
    },
    askRows: async (runIds) => {
      const before = get().listed;
      set({ listed: new Set(runIds) });
      await answer(runIds.filter((id) => !before.has(id)).map(rowQuestion));
    },
    refresh: async () => {
      const { viewed, listed } = get();
      await answer([...(viewed ? [viewed] : []), ...[...listed].map(rowQuestion)]);
    },
  };
});

/** Ask again every gesture asked: the rights or the Projets may have changed. */
export function refreshGestureRights(): void {
  void useGestureRightsStore.getState().refresh();
}

/**
 * Ask the gestures again whenever the window comes back: a right changed
 * elsewhere (an administrator's gesture) reaches a tab left open.
 */
export function useRefreshGestureRightsOnReturn(): void {
  useEffect(() => {
    const whenVisible = () => {
      if (document.visibilityState === "visible") refreshGestureRights();
    };
    window.addEventListener("focus", refreshGestureRights);
    document.addEventListener("visibilitychange", whenVisible);
    return () => {
      window.removeEventListener("focus", refreshGestureRights);
      document.removeEventListener("visibilitychange", whenVisible);
    };
  }, []);
}

/**
 * The view of the Run `runId` asks what it offers: once when it opens, and
 * again when its nodes change (not on every refresh of the Run).
 */
export function useRunViewGestures(runId: string | null, nodeIds: readonly string[]): void {
  const askView = useGestureRightsStore((s) => s.askView);
  const nodesKey = [...nodeIds].sort().join("\n");
  useEffect(() => {
    if (runId) void askView(runId, nodesKey ? nodesKey.split("\n") : []);
  }, [runId, nodesKey, askView]);
}

/**
 * The shell's open Run, `run`: its view asks what it offers when it opens,
 * and every gesture asked is asked again when the window comes back.
 */
export function useSelectedRunGestures(run: RunState | null): void {
  useRunViewGestures(run?.run_id ?? null, Object.keys(run?.nodes ?? {}));
  useRefreshGestureRightsOnReturn();
}

/** The rows of the Runs list ask what they offer, once per Run. */
export function useListedRunGestures(runIds: readonly string[]): void {
  const askRows = useGestureRightsStore((s) => s.askRows);
  const key = runIds.join("\n");
  useEffect(() => {
    void askRows(key ? key.split("\n") : []);
  }, [key, askRows]);
}

/**
 * Why the caller may not make a gesture on the Run `runId`: the sentence a
 * greyed gesture's tooltip says (`disabledReason`), `undefined` when allowed.
 */
export function useWhyNotOnRun(runId: string | null | undefined): (gesture: RunGesture) => string | undefined {
  const rights = useGestureRightsStore((s) => s.rights);
  return useCallback(
    (gesture: RunGesture) => (runId ? whyNotOnRun(rights, runId, gesture) : undefined),
    [rights, runId],
  );
}

/** Why the caller may not make each gesture on the node `nodeId` of `runId`, or `undefined`. */
export function useWhyNotOnNode(runId: string, nodeId: string): (gesture: NodeGesture) => string | undefined {
  const rights = useGestureRightsStore((s) => s.rights);
  return useCallback(
    (gesture: NodeGesture) => whyNotOnNode(rights, runId, nodeId, gesture),
    [rights, runId, nodeId],
  );
}

