import type { StatsCostEntity } from "../../../../shared/generated/StatsCostEntity";
import type { StatsCostRollUp } from "../../../../shared/generated/StatsCostRollUp";
import type { StatsCostDepth } from "../../../../shared/generated/StatsCostDepth";
import { plural } from "../../../../shared/lib/plural";
import type { CostTree } from "./costTree";
import { RollUpTag } from "./RollUpMarks";

/**
 * « Roll up child runs » in the Cost drill: inside a root Pipeline whose row
 * carries a roll-up, Parent run | Child runs › Depth k › child Pipeline ›
 * Nodes, in the same table and the same breadcrumb as the rest of the drill.
 */

/** Where the drill stands inside a root Pipeline's roll-up. */
export interface RollUpPath {
  branch: "parent" | "children" | null;
  depth: number | null;
  pipelineId: string | null;
}

export const ROLL_UP_ROOT: RollUpPath = {
  branch: null,
  depth: null,
  pipelineId: null,
};

const isDepth = (row: StatsCostEntity): row is StatsCostDepth =>
  "depth" in row && "pipelines" in row;

/** One level of the roll-up drill, resolved from where it stands. */
export interface RollUpLevel {
  /** The row the headline and the period bars read; `null` on the root
   *  Pipeline itself (its own row). */
  focus: StatsCostEntity | null;
  /** The Pipeline whose Nodes the table shows, with those Nodes: the root's
   *  own under Parent run, a child Pipeline's under its depth. */
  nodeLevel: { pipeline: StatsCostEntity; nodes: StatsCostEntity[] } | null;
  /** The table's rows when it shows no Nodes. */
  rows: StatsCostEntity[];
  /** The rows unfold in place: Parent run → its Nodes, Child runs → its
   *  depths, a depth → its Pipelines, a Pipeline → its Nodes. None on a Node
   *  level, whose rows unfold their model × effort pairs. */
  tree: CostTree | undefined;
  /** The crumbs after the root Pipeline's. */
  crumbs: { label: string; onClick?: () => void }[];
}

export function resolveRollUp(
  pipeline: StatsCostEntity,
  rollUp: StatsCostRollUp,
  path: RollUpPath,
  setPath: (path: RollUpPath) => void,
): RollUpLevel {
  const depth =
    path.branch === "children" && path.depth !== null
      ? (rollUp.depths.find((level) => level.depth === path.depth) ?? null)
      : null;
  const childPipeline =
    depth && path.pipelineId !== null
      ? (depth.pipelines.find((row) => row.id === path.pipelineId) ?? null)
      : null;

  // The ancestors of the rows the table lists, at this level of the drill.
  const base: StatsCostEntity[] =
    path.branch !== "children" ? [] : depth ? [rollUp.children, depth] : [rollUp.children];
  const tree: CostTree = {
    base,
    children: (row, ancestors) => {
      if (row.id === rollUp.parent.id && ancestors.length === 0) return rollUp.parent.nodes;
      if (row.id === rollUp.children.id && ancestors.length === 0) return rollUp.depths;
      if (isDepth(row)) return row.pipelines;
      const parent = ancestors[ancestors.length - 1];
      return parent && isDepth(parent) ? row.nodes : undefined;
    },
    open: (row, ancestors) => {
      const parent = ancestors[ancestors.length - 1];
      if (ancestors.length === 0 && row.id === rollUp.parent.id)
        return () => setPath({ branch: "parent", depth: null, pipelineId: null });
      if (ancestors.length === 0 && row.id === rollUp.children.id)
        return () => setPath({ branch: "children", depth: null, pipelineId: null });
      if (isDepth(row))
        return () => setPath({ branch: "children", depth: row.depth, pipelineId: null });
      if (parent && isDepth(parent))
        return () =>
          setPath({ branch: "children", depth: parent.depth, pipelineId: row.id });
      return undefined;
    },
    // A Node row sits under Parent run, or under a child Pipeline of a depth.
    unit: (_row, ancestors) => {
      const parent = ancestors[ancestors.length - 1];
      const grandparent = ancestors[ancestors.length - 2];
      return parent?.id === rollUp.parent.id || (grandparent && isDepth(grandparent))
        ? "execution"
        : "Run";
    },
    renderName: (row) => (
      <>
        <span className="sx-ellipsis" title={row.name}>
          {row.name}
        </span>
        {row.id === rollUp.parent.id ? (
          <RollUpTag>this run&apos;s own nodes</RollUpTag>
        ) : row.id === rollUp.children.id ? (
          <RollUpTag>
            {plural(rollUp.children.runs ?? 0, "run")} · {plural(rollUp.levels, "level")}
          </RollUpTag>
        ) : isDepth(row) ? (
          <RollUpTag>{plural(row.runs ?? 0, "run")}</RollUpTag>
        ) : null}
      </>
    ),
    // Child runs and its first depth open, so the split by depth reads at once.
    expanded: [
      rollUp.children.id,
      ...rollUp.depths.slice(0, 1).map((level) => `${rollUp.children.id}/${level.id}`),
    ],
  };

  const crumbs: RollUpLevel["crumbs"] = [];
  if (path.branch === "parent") crumbs.push({ label: rollUp.parent.name });
  if (path.branch === "children")
    crumbs.push({
      label: rollUp.children.name,
      onClick: () => setPath({ branch: "children", depth: null, pipelineId: null }),
    });
  if (depth)
    crumbs.push({
      label: depth.name,
      onClick: () => setPath({ ...path, pipelineId: null }),
    });
  if (childPipeline) crumbs.push({ label: childPipeline.name });

  if (path.branch === "parent")
    return {
      focus: rollUp.parent,
      nodeLevel: { pipeline, nodes: rollUp.parent.nodes },
      rows: rollUp.parent.nodes,
      tree: undefined,
      crumbs,
    };
  if (childPipeline)
    return {
      focus: childPipeline,
      nodeLevel: { pipeline: childPipeline, nodes: childPipeline.nodes },
      rows: childPipeline.nodes,
      tree: undefined,
      crumbs,
    };
  if (depth)
    return { focus: depth, nodeLevel: null, rows: depth.pipelines, tree, crumbs };
  if (path.branch === "children")
    return { focus: rollUp.children, nodeLevel: null, rows: rollUp.depths, tree, crumbs };
  return {
    focus: null,
    nodeLevel: null,
    rows: [rollUp.parent, rollUp.children],
    tree,
    crumbs,
  };
}
