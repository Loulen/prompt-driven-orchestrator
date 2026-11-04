/**
 * What the previews of the Run canvas and Run info blocks of Settings ›
 * Appearance show: the most recent root Run the user sees (`recentRootRun`),
 * or a sample when they see none.
 *
 * The canvas preview draws a few of its cards in a fixed shape, so a fork and
 * a join show that no edge gains a bend as the cards shorten: one card, a fork
 * of two, then an orchestrator with child runs when the Run has one, then one
 * more. A Run with fewer cards is drawn as a chain.
 */
import type { EditNodeData } from "../../pipeline/components/EditNode";
import type { EditorPipelineDef } from "../../pipeline/model/pipeline";
import type { NodeCost } from "../../shared/generated/NodeCost";
import type { RunListEntry } from "../../shared/generated/RunListEntry";
import type { RunState } from "../../shared/generated/RunState";
import { mostRecentRun } from "./runsListPreview";

/**
 * The Run the previews draw: the most recent Run that no other Run started
 * (`mostRecentRun` among them), since an orchestrator's child, often newer than its
 * parent, is a single node with no fork. Any most recent Run when every Run has a parent.
 */
export function recentRootRun(runs: RunListEntry[]): RunListEntry | null {
  const roots = runs.filter((run) => !run.parent_run_id);
  return mostRecentRun(roots.length > 0 ? roots : runs);
}

/** The rows of the canvas preview, top to bottom: one card, or two side by side (a fork). */
export function canvasPreviewRows(cards: readonly EditNodeData[]): EditNodeData[][] {
  const work = cards.filter((card) => card.nodeType !== "start" && card.nodeType !== "end");
  const orchestrator = work.find((card) => card.childCounts != null);
  // A card with child runs is taller: one never sits in the fork, beside a shorter one.
  const rest = work.filter((card) => card.childCounts == null);
  if (rest.length < 3) return [...rest, ...(orchestrator ? [orchestrator] : [])].map((card) => [card]);
  const rows: EditNodeData[][] = [[rest[0]], [rest[1], rest[2]]];
  const after = rest.slice(3);
  const join = orchestrator ?? after.shift();
  if (join) rows.push([join]);
  if (after.length > 0) rows.push([after[0]]);
  return rows;
}

const cost = (usd: number): NodeCost =>
  ({ usd, form: "reported", reported_in_usd: true, partial: false, executions: 1, readable_executions: 1 }) as NodeCost;

const at = (minutes: number) => new Date(Date.UTC(2026, 0, 1, 10, minutes)).toISOString();

function card(over: Partial<EditNodeData> & Pick<EditNodeData, "nodeId">): EditNodeData {
  return {
    label: over.nodeId,
    nodeType: "agent",
    interactive: false,
    status: "completed",
    inputs: [],
    outputs: [],
    inRun: true,
    agent: { short: "Sonnet", title: "Agent" },
    ...over,
  };
}

/** The sample Run's cards, drawn when the user sees no Run: no extension mark goes on them. */
export const SAMPLE_CARDS: readonly EditNodeData[] = [
  card({
    nodeId: "plan",
    interactive: true,
    agentMode: "profile",
    run: { iter: 1, iterations: 1, startedAt: at(0), completedAt: at(3), cost: cost(0.31) },
  }),
  card({
    nodeId: "tdd",
    agent: { short: "Opus", title: "Agent" },
    isolated: true,
    loopBadge: { text: "×2", kind: "bounded" },
    run: { iter: 2, iterations: 2, startedAt: at(3), completedAt: at(10), cost: cost(1.24) },
  }),
  card({ nodeId: "docs", run: { iter: 1, iterations: 1, startedAt: at(3), completedAt: at(5), cost: cost(0.18) } }),
  card({
    nodeId: "agentic tests",
    status: "running",
    orchestrator: true,
    childCounts: { running: 1, failed: 0, stale: 0, finished: 2 },
    run: { iter: 1, iterations: 1, startedAt: at(10), completedAt: at(14), cost: cost(0.68) },
  }),
  card({ nodeId: "merge", status: "pending", run: { iter: 0, iterations: 0, startedAt: null, completedAt: null, cost: null } }),
];

/** The sample Run's state, for the Info tab's preview: started 14 minutes before `now`. */
export function sampleRunState(now: number): RunState {
  return {
    run_id: "sample-run",
    name: "Sample run",
    status: "running",
    pipeline_name: "my-pipeline",
    input: null,
    harness: "claude-code",
    target_repo: "my-repo",
    sandbox: "full",
    started_at: new Date(now - 14 * 60_000).toISOString(),
    completed_at: null,
    sessions_spawned: 5,
    loc: { insertions: 482, deletions: 37, files_changed: 9 },
    cost: { usd: 2.41, partial: false, unpriced_models: [], uncosted_harnesses: [], by_harness: [] },
    nodes: {},
    edges: [],
    node_defs: [],
    loop_states: {},
    foreach_states: {},
    collection_states: {},
    switch_states: {},
  } as unknown as RunState;
}

/** The sample Run's pipeline, for the Info tab's preview. */
export const SAMPLE_PIPELINE: EditorPipelineDef = {
  name: "my-pipeline",
  version: "1.0",
  variables: {},
  nodes: [],
  edges: [],
};
