import { Info } from "lucide-react";
import type { StatsCostEntity } from "../../../../shared/generated/StatsCostEntity";
import { formatCostAmount } from "../../../../run/model/costLabel";
import { Tooltip } from "../../../../shared/ui/tooltip";
import type { MasterSelection } from "../../../store/absorptionSelection";
import type { CostAxis } from "../../../model/cost";
import { Panel } from "../common/ChartPrimitives";
import { MasterList } from "../common/MasterList";

/** The Cost ranking: a panel with the grouping select in its head (and, on
 *  « By model », an « i » for how the axis reads), the master list under it. */
export function CostRanking({
  axis,
  onAxisChange,
  noCostSource,
  rows,
  selected,
  selection,
  onSelect,
  badge,
}: {
  axis: CostAxis;
  onAxisChange: (axis: CostAxis) => void;
  /** The harnesses without a cost source, left off the « By model » axis. */
  noCostSource: string[];
  rows: StatsCostEntity[];
  selected: string | null;
  selection: MasterSelection | undefined;
  onSelect: (id: string | null) => void;
  /** A discreet mark after a row's name (the « ⑂ N » of a root Pipeline). */
  badge?: (row: StatsCostEntity) => React.ReactNode;
}) {
  const axisNote = [
    "Model ids verbatim, one row per id — the same id run through two harnesses is one row, one column per harness. Hover a model or an effort for where the value was read and its provider.",
    noCostSource.length > 0
      ? `${noCostSource.join(", ")} has no cost source and is not on this axis.`
      : "",
  ]
    .filter(Boolean)
    .join(" ");
  return (
    <Panel
      title="Ranked by cost"
      flush
      className="sx-aside"
      testid="stats-drilldown-navigation"
      aside={
        <>
          {axis === "model" && (
            <Tooltip content={axisNote} side="top">
              <span role="img" aria-label={axisNote} className="inline-flex text-text-3">
                <Info size={12} strokeWidth={1.5} />
              </span>
            </Tooltip>
          )}
          <select
            aria-label="Cost grouping"
            value={axis}
            onChange={(event) => onAxisChange(event.target.value as CostAxis)}
            className="sx-select"
          >
            <option value="pipeline">By pipeline</option>
            <option value="project">By project</option>
            <option value="model">By model</option>
          </select>
        </>
      }
    >
      <MasterList
        rows={rows}
        selected={selected}
        monoName={axis === "model"}
        valueLabel={(row) => formatCostAmount(row.usd, row.partial, row.estimated)}
        selection={selection}
        onSelect={onSelect}
        badge={badge}
      />
    </Panel>
  );
}
