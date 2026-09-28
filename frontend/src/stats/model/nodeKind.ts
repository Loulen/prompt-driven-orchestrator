import type { StatsPerformanceEntity } from "../../shared/generated/StatsPerformanceEntity";
import type { NodeKind } from "./statsFilters";
import type { PerformanceAxis } from "./performance";

// --- Node kind (#810, CONTEXT.md § Genre de nœud) ------------------------------

/** Does this row carry a kind? Only Node rows do — a Pipeline, a subagent
 *  group, an Infrastructure role and every level of the « By model » tree
 *  answer `false` and are never touched by the filter. */
export function isNodeRow(row: StatsPerformanceEntity): boolean {
  return row.interactive !== undefined || row.orchestrator !== undefined;
}

/** The kinds a Node row matches. Both flags count: a node that is interactive
 *  AND orchestrator appears under either chip, so no execution is hidden by an
 *  exclusive classification. */
export function rowKinds(row: StatsPerformanceEntity): NodeKind[] {
  const kinds: NodeKind[] = [];
  if (row.interactive) kinds.push("interactive");
  if (row.orchestrator) kinds.push("orchestrator");
  if (kinds.length === 0) kinds.push("standard");
  return kinds;
}

export function matchesKinds(row: StatsPerformanceEntity, kinds: NodeKind[]): boolean {
  if (!isNodeRow(row)) return true;
  return rowKinds(row).some((kind) => kinds.includes(kind));
}

/** How many Node rows of the whole period carry each kind — the counts on the
 *  chips. Read from the Pipeline tree, the canonical node population; the
 *  « By model » axis re-buckets those same executions. */
export function nodeKindCounts(
  rows: StatsPerformanceEntity[],
): Record<NodeKind, number> {
  const counts: Record<NodeKind, number> = {
    interactive: 0,
    orchestrator: 0,
    standard: 0,
  };
  for (const pipeline of rows) {
    for (const node of pipeline.nodes) {
      for (const kind of rowKinds(node)) counts[kind] += 1;
    }
  }
  return counts;
}

/** Does the kind filter hide at least one Node of this pipeline? A pipeline
 *  whose every node is visible keeps its own total; one that lost a node shows
 *  « filtered ». */
export function hasHiddenNode(
  pipeline: StatsPerformanceEntity,
  kinds: NodeKind[],
): boolean {
  return pipeline.nodes.some((node) => !matchesKinds(node, kinds));
}

/** Is what the head cards, the headline and the Steered card summarise still
 *  the population on screen? Infrastructure is never filtered by kind, so a
 *  selected Infrastructure row always reads its own true numbers. */
export function partialByKindAt(
  axis: PerformanceAxis,
  selected: StatsPerformanceEntity | null,
  anyHidden: boolean,
  kinds: NodeKind[],
): boolean {
  return axis === "model"
    ? anyHidden
    : selected === null
      ? anyHidden
      : selected.id === "__infrastructure__"
        ? false
        : hasHiddenNode(selected, kinds);
}

/** Rows whose own aggregate no longer describes what is visible under them.
 *  Never a value recomputed from the visible nodes' six stats: a median of
 *  medians is not a median (spec #809). */
export function filteredRowIdsAt({
  axis,
  byPipeline,
  detailRows,
  kinds,
  kindFilterActive,
  anyHidden,
}: {
  axis: PerformanceAxis;
  byPipeline: StatsPerformanceEntity[];
  detailRows: StatsPerformanceEntity[];
  kinds: NodeKind[];
  kindFilterActive: boolean;
  anyHidden: boolean;
}): Set<string> {
  const filteredRowIds = new Set<string>();
  if (kindFilterActive) {
    if (axis === "pipeline") {
      for (const row of byPipeline) {
        if (hasHiddenNode(row, kinds)) filteredRowIds.add(row.id);
      }
    } else if (anyHidden) {
      for (const row of detailRows) {
        if (!isNodeRow(row)) filteredRowIds.add(row.id);
      }
    }
  }
  return filteredRowIds;
}
