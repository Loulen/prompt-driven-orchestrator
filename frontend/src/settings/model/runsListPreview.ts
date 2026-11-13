/**
 * What the preview of the Runs list block of Settings › Appearance shows: the
 * most recent Run the user sees, the most recent triggered Run when none of the
 * other rows has a Trigger, the most recent Run from an External source when
 * none of them has one, and a parent with its child, so that every element of
 * a row (External source, Trigger mark, child pills, parent link) appears once;
 * a sample Run when the user sees none.
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
    // `#/` is no deep link: `followDeepLink` ignores it, so the sample's chip opens nothing.
    provenance: { label: "Sample source", link: "#/", extension: "sample" },
    effective_repo: "",
    nodes: [
      { id: "plan", status: "completed" },
      { id: "build", status: "running" },
      { id: "review", status: "pending" },
    ],
  };
}

/** Newest first: a Run not started yet is the newest; then by start. */
export function newer(a: RunListEntry, b: RunListEntry): number {
  if (a.started_at === b.started_at) return 0;
  if (a.started_at === null) return -1;
  if (b.started_at === null) return 1;
  return a.started_at < b.started_at ? 1 : -1;
}

function everyNode(tree: RunTreeNode[]): RunTreeNode[] {
  return tree.flatMap((node) => [node, ...everyNode(node.children)]);
}

/** The most recent Run the user sees, an active one before an archived one; `null` with none. */
export function mostRecentRun(runs: RunListEntry[]): RunListEntry | null {
  const active = runs.filter((run) => run.status !== "archived");
  return [...(active.length > 0 ? active : runs)].sort(newer)[0] ?? null;
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
  const sorted = [...nodes].sort(byNewest);
  const recent = sorted[0];
  const parent = sorted.find((node) => node.children.length > 0);
  // The child names its parent: the Parent link shows once, when it is shown.
  const child = parent && [...parent.children].sort(byNewest)[0];
  const family =
    parent && child
      ? [
          { ...parent, depth: 0 },
          { ...child, depth: 1, sectionParentLink: parent.run.run_id },
        ]
      : [];
  const inFamily = (node: RunTreeNode) => family.some((member) => member.run.run_id === node.run.run_id);
  const rows = inFamily(recent) ? [] : [{ ...recent, depth: 0 }];
  // The Trigger mark shows once too: the most recent triggered Run joins when no row has one.
  const triggered = sorted.find((node) => node.run.triggered_by);
  const hasTrigger = [...rows, ...family].some((node) => node.run.triggered_by);
  const addTriggered = triggered !== undefined && !hasTrigger;
  if (addTriggered) rows.push({ ...triggered, depth: 0 });
  // The External source too: the most recent Run with one joins when no row has one.
  const sourced = sorted.find((node) => node.run.provenance);
  const hasSource = [...rows, ...family].some((node) => node.run.provenance);
  const addSourced = sourced !== undefined && !hasSource;
  if (addSourced) rows.push({ ...sourced, depth: 0 });
  const parts = [
    "Your most recent run",
    ...(addTriggered ? ["a triggered run"] : []),
    ...(addSourced ? ["a run from an external source"] : []),
    ...(family.length > 0 ? ["a parent with its child"] : []),
  ];
  const caption = parts.length === 1 ? parts[0] : `${parts.slice(0, -1).join(", ")}, and ${parts[parts.length - 1]}`;
  return { nodes: [...rows, ...family], sample: false, caption };
}
