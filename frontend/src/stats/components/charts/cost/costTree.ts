import type { StatsCostEntity } from "../../../../shared/generated/StatsCostEntity";

/**
 * Rows of a Cost table that unfold in place, level under level — the
 * « Roll up child runs » split of a root Pipeline: Parent run → its Nodes,
 * Child runs → Depth k → child Pipeline → Nodes. Each row also opens its level
 * of the drill, like any row of the table.
 */
export interface CostTree {
  /** The ancestors of the rows the table lists, at this level of the drill. */
  base: StatsCostEntity[];
  /** The rows under `row`, given its ancestors; none for a leaf. */
  children: (row: StatsCostEntity, ancestors: StatsCostEntity[]) => StatsCostEntity[] | undefined;
  /** What opening `row` does in the drill; none when it opens nothing. */
  open: (row: StatsCostEntity, ancestors: StatsCostEntity[]) => (() => void) | undefined;
  /** What a row's figures count: a Node row executions, the others Runs. */
  unit: (row: StatsCostEntity, ancestors: StatsCostEntity[]) => "Run" | "execution";
  /** The name cell of a row of the tree. */
  renderName: (row: StatsCostEntity) => React.ReactNode;
  /** The rows open at first, by key ({@link treeKey}). */
  expanded: string[];
}

/** A row's key in the tree: its ancestors' ids and its own. */
export const treeKey = (ancestors: StatsCostEntity[], row: StatsCostEntity) =>
  [...ancestors, row].map((item) => item.id).join("/");
