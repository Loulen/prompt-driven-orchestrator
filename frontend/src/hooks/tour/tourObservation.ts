/**
 * What the tour hook observes (#823, #825): the app state a step's predicates
 * read, built from the edit store and the Runs the UI already loaded, and the
 * DOM narrowed to the three questions a {@link TourObservation} asks.
 */

import { useEditStore } from "../../pipeline/store/editStore";
import type { TourAppState, TourObservation, TourRunNode } from "../../lib/tourDef";
import { visible } from "./tourGeometry";

/** One entry of the Run list, narrowed to what a tour reads. */
export interface TourRunEntry {
  run_id: string;
  name?: string | null;
  pipeline_name?: string;
  status?: string;
}

/**
 * The Runs the tour is allowed to see — the same list the left panel renders.
 * The shape is narrowed here so the hook never depends on the full `RunListEntry`.
 */
export type TourRunsInput = ArrayLike<TourRunEntry>;

/**
 * The selected Run's detail, narrowed to what the reading steps read (#825) —
 * the shape `RunState` already has, minus everything else, so the hook never
 * depends on the full projection. `null` while no Run is open.
 */
export interface TourRunDetailInput {
  run_id: string;
  nodes: Record<
    string,
    {
      node_id: string;
      status: string;
      iter: number;
      iterations: { iter: number; completion_released?: boolean }[];
    }
  >;
  node_defs?: { id: string; name?: string | null; node_type: string }[];
}

/**
 * The Run's nodes, in the order its frozen pipeline declares them, each carrying
 * its status and whether the human has opened its completion guard (ADR-0068).
 *
 * The release flag lives on the *current iteration*, not on the node: a retried
 * node is guarded again, and a tour that read the first iteration's flag would
 * tell the user they had already released a node that is waiting for them.
 */
function runNodes(detail: TourRunDetailInput): TourRunNode[] {
  const defs = detail.node_defs ?? [];
  const order = defs.length > 0 ? defs.map((d) => d.id) : Object.keys(detail.nodes);
  return order.flatMap((id) => {
    const node = detail.nodes[id];
    if (!node) return [];
    const def = defs.find((d) => d.id === id);
    const iteration = node.iterations.find((it) => it.iter === node.iter);
    return [
      {
        id,
        name: def?.name?.trim() || id,
        type: def?.node_type ?? "agent",
        status: node.status,
        released: iteration?.completion_released === true,
      },
    ];
  });
}

export function readTourAppState(
  runs: TourRunsInput,
  detail: TourRunDetailInput | null = null,
  launchedRunId: string | null = null,
): TourAppState {
  const s = useEditStore.getState();
  const tab = s.openTabs.find((t) => t.id === s.activeTabId) ?? null;
  // Your Run is the one your Launch got back, never `runs[0]`: on a shared
  // instance the head of the list is whoever launched last.
  const mine = launchedRunId
    ? (Array.from(runs).find((r) => r.run_id === launchedRunId) ?? null)
    : null;
  return {
    pipelineId: tab?.id ?? null,
    pipeline: tab?.pipeline ?? null,
    prompts: tab?.prompts ?? {},
    selection: {
      kind: s.selection.kind,
      id: s.selection.id,
      edgeIndex: s.selection.edgeIndex ?? null,
    },
    dirty: tab?.dirty ?? false,
    libraryPipelineIds: s.pipelines.map((p) => p.id),
    runCount: runs.length,
    runs: Array.from(runs, (r) => ({
      id: r.run_id,
      name: r.name?.trim() || r.run_id,
      pipeline: r.pipeline_name ?? "",
      status: r.status ?? "",
    })),
    launchedRunId,
    myRun: launchedRunId
      ? {
          id: launchedRunId,
          // The list refreshes a beat after the Launch answered: until it has,
          // the id is the only name there is.
          name: mine?.name?.trim() || launchedRunId,
          // Only the detail OF THAT Run: the inspector may well be showing
          // another one, and a tour reading a stranger's nodes would advance on
          // somebody else's progress.
          nodes: detail?.run_id === launchedRunId ? runNodes(detail) : [],
        }
      : null,
    activeRunId: tab?.runId ?? null,
  };
}

export function observation(
  runs: TourRunsInput,
  detail: TourRunDetailInput | null,
  launchedRunId: string | null,
  baseline: TourAppState,
): TourObservation {
  return {
    app: readTourAppState(runs, detail, launchedRunId),
    baseline,
    present: (selector) => {
      try {
        return visible(document.querySelector(selector));
      } catch {
        return false;
      }
    },
    value: (selector) => {
      try {
        const el = document.querySelector(selector);
        if (el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement) return el.value;
        return null;
      } catch {
        return null;
      }
    },
    text: (selector) => {
      try {
        const el = document.querySelector(selector);
        // An element that is in the document but has no box is not showing
        // anything to anyone — same rule as `present`.
        return visible(el) ? (el.textContent?.trim() ?? null) || null : null;
      } catch {
        return null;
      }
    },
  };
}
