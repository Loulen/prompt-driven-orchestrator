import type { StatsCost } from "../../../../shared/generated/StatsCost";
import type { StatsCostEntity } from "../../../../shared/generated/StatsCostEntity";
import type { StatsModelCostEntity } from "../../../../shared/generated/StatsModelCostEntity";
import type { MasterSelection } from "../../../store/absorptionSelection";
import type { useStatsAbsorption } from "../../../store/useStatsAbsorption";
import {
  coupleRows,
  coupleScope,
  hasExplicitEffort,
} from "../../../model/statsAbsorption";
import { isCostNodeRow, type CostAxis } from "../../../model/cost";
import { RUNS_COUNT } from "../../../model/rowCounts";
import type { CoupleSelection } from "../common/tableSelection";

/** The Nodes the table shows, and the Pipeline they belong to — reached on
 *  whichever axis, under a roll-up or not. */
export interface NodeLevel {
  pipeline: StatsCostEntity;
  nodes: StatsCostEntity[];
}

/**
 * Which rows of the Cost tab combine (#890 / #892 / #906): Pipelines on
 * « By pipeline », Models on « By model », the Nodes of the Pipeline on screen
 * on every axis — never the Projects —, the efforts of one model, and the
 * couples of one Node.
 */
export function costSelections(
  absorption: ReturnType<typeof useStatsAbsorption>,
  {
    axis,
    cost,
    nodeLevel,
    model,
    effortLevel,
  }: {
    axis: CostAxis;
    cost: StatsCost;
    nodeLevel: NodeLevel | null;
    model: StatsModelCostEntity | null;
    /** The model's efforts are on screen (no effort drilled yet). */
    effortLevel: boolean;
  },
): {
  master: MasterSelection | undefined;
  node: MasterSelection | undefined;
  effort: MasterSelection | undefined;
  couple: CoupleSelection | undefined;
} {
  const master =
    axis === "pipeline"
      ? absorption.selectionFor({ dimension: "pipeline", rows: cost.by_pipeline, count: RUNS_COUNT })
      : axis === "model"
        ? absorption.selectionFor({ dimension: "model", rows: cost.by_model, count: RUNS_COUNT })
        : undefined;
  const node = nodeLevel
    ? absorption.selectionFor({
        dimension: "node",
        scope: { key: nodeLevel.pipeline.id, name: nodeLevel.pipeline.name },
        rows: nodeLevel.nodes,
        count: RUNS_COUNT,
        selectable: isCostNodeRow,
      })
    : undefined;
  const effort =
    model && effortLevel
      ? absorption.selectionFor({
          dimension: "effort",
          scope: { key: model.id, name: model.name },
          rows: model.efforts,
          count: RUNS_COUNT,
          preferred: hasExplicitEffort,
        })
      : undefined;
  const couple: CoupleSelection | undefined = nodeLevel
    ? (row) =>
        absorption.selectionFor({
          dimension: "couple",
          scope: {
            key: coupleScope(nodeLevel.pipeline.id, row.id),
            name: `${row.name} (${nodeLevel.pipeline.name})`,
          },
          rows: coupleRows(nodeLevel.nodes.find((item) => item.id === row.id)?.models ?? []),
          count: RUNS_COUNT,
          // Infrastructure and Unassigned are no Node: their couples show the
          // global mark, never a ring.
          selectable: () => isCostNodeRow(row as StatsCostEntity),
          preferred: hasExplicitEffort,
        })
    : undefined;
  return { master, node, effort, couple };
}
