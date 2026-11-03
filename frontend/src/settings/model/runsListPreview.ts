/**
 * What the preview of the Runs list block of Settings › Appearance shows: the
 * most recent Run the user sees, and a parent with its child so that every
 * element of a row (child pills, parent link) appears once; a sample Run when
 * the user sees none.
 */
import type { RunListEntry } from "../../shared/generated/RunListEntry";
import { buildRunTree, type RunTreeNode } from "../../run/model/runTree";
import type { ChildCounts } from "../../run/model/orchestration";

export interface RunsListPreview {
  /** The rows, in their order, each at its depth in the preview. */
  nodes: RunTreeNode[];
  /** A sample Run: the user sees no Run. */
  sample: boolean;
  caption: string;
  /** The sample's own cost. */
  sampleCost?: number;
}

const NO_CHILD: ChildCounts = { running: 0, failed: 0, stale: 0, finished: 0 };

/** The Run the preview shows when the user sees none: no extension mark goes on it. */
export function sampleRun(now: number): RunListEntry {
  return {
    run_id: "sample-run",
    name: "Sample run",
    pipeline_name: "my-pipeline",
    status: "running",
    stalled: false,
    started_at: new Date(now - 6 * 60_000).toISOString(),
    triggered_by: "Sample trigger",
    effective_repo: "",
    nodes: [
      { id: "plan", status: "completed" },
      { id: "build", status: "running" },
      { id: "review", status: "pending" },
    ],
  };
}

/** Newest first: a Run not started yet is the newest; then by start. */
function newer(a: RunListEntry, b: RunListEntry): number {
  if (a.started_at === b.started_at) return 0;
  if (a.started_at === null) return -1;
  if (b.started_at === null) return 1;
  return a.started_at < b.started_at ? 1 : -1;
}

function everyNode(tree: RunTreeNode[]): RunTreeNode[] {
  return tree.flatMap((node) => [node, ...everyNode(node.children)]);
}

/** The rows of the preview, from the Runs the user sees. */
export function runsListPreview(runs: RunListEntry[], now: number = Date.now()): RunsListPreview {
  if (runs.length === 0) {
    const run = sampleRun(now);
    return {
      nodes: [{ run, children: [], depth: 0, counts: NO_CHILD, sectionParentLink: null }],
      sample: true,
      caption: "Sample run · you see no run yet",
      sampleCost: 0.42,
    };
  }
  // The active Runs first: an archived one only when the user sees nothing else.
  const active = runs.filter((run) => run.status !== "archived");
  const nodes = everyNode(buildRunTree(active.length > 0 ? active : runs));
  const byNewest = (a: RunTreeNode, b: RunTreeNode) => newer(a.run, b.run);
  const recent = [...nodes].sort(byNewest)[0];
  const parent = nodes.filter((node) => node.children.length > 0).sort(byNewest)[0];
  if (!parent) {
    return { nodes: [{ ...recent, depth: 0 }], sample: false, caption: "Your most recent run" };
  }
  const child = [...parent.children].sort(byNewest)[0];
  // The child names its parent: the Parent link shows once, when it is shown.
  const family = [
    { ...parent, depth: 0 },
    { ...child, depth: 1, sectionParentLink: parent.run.run_id },
  ];
  const inFamily = recent === parent || recent === child;
  return {
    nodes: inFamily ? family : [{ ...recent, depth: 0 }, ...family],
    sample: false,
    caption: "Your most recent run, and a parent with its child",
  };
}
