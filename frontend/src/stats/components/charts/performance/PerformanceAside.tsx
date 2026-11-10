import type { StatsPerformanceEntity } from "../../../../shared/generated/StatsPerformanceEntity";
import type { MasterSelection } from "../../../store/absorptionSelection";
import type { DurationMode } from "../../../model/statsFilters";
import {
  durationWord,
  formatPerformanceValue,
  performanceScore,
  performanceSortLabel,
  type PerformanceAxis,
  type PerformanceMetric,
} from "../../../model/performance";
import { Info } from "lucide-react";
import { Tooltip } from "../../../../shared/ui/tooltip";
import { Panel } from "../common/ChartPrimitives";
import { MasterList } from "../common/MasterList";
import { FilteredValue } from "./NodeKindMarks";

/** The Performance ranking: a panel with the grouping and sort selects (#737)
 *  in its head, the master list under it. */
export function PerformanceAside({
  axis,
  onAxisChange,
  sort,
  onSortChange,
  mode,
  masterRows,
  masterSelection,
  selected,
  filteredRowIds,
  anyHidden,
  onSelect,
}: {
  axis: PerformanceAxis;
  onAxisChange: (axis: PerformanceAxis) => void;
  sort: PerformanceMetric;
  onSortChange: (sort: PerformanceMetric) => void;
  mode: DurationMode;
  masterRows: StatsPerformanceEntity[];
  masterSelection: MasterSelection | undefined;
  selected: string | null;
  filteredRowIds: Set<string>;
  anyHidden: boolean;
  onSelect: (id: string | null) => void;
}) {
  const axisNote =
    "Model ids verbatim, one row per id — the same id run through two harnesses is one row, one column per harness. Hover a model or an effort for where the value was read.";
  return (
    <Panel
      title={`Ranked by ${performanceSortLabel(sort, mode)} (median)`}
      flush
      className="sx-aside"
    >
      <div className="sx-panel__tools">
          {axis === "model" && (
            <Tooltip content={axisNote} side="top">
              <span role="img" aria-label={axisNote} className="inline-flex text-text-3">
                <Info size={12} strokeWidth={1.5} />
              </span>
            </Tooltip>
          )}
          <select
            aria-label="Performance grouping"
            value={axis}
            onChange={(event) =>
              onAxisChange(event.target.value as PerformanceAxis)
            }
            className="sx-select"
          >
            <option value="pipeline">By pipeline</option>
            <option value="model">By model</option>
          </select>
          <select
            aria-label="Performance sort"
            value={sort}
            onChange={(event) =>
              onSortChange(event.target.value as PerformanceMetric)
            }
            className="sx-select"
          >
            <option value="context">By context</option>
            <option value="duration">By {durationWord(mode)}</option>
            <option value="steering">By steering</option>
          </select>
      </div>
      <MasterList
        rows={masterRows}
        selection={masterSelection}
        selected={selected}
        monoName={axis === "model"}
        ariaLabel="Performance groups"
        valueLabel={(row) => {
          if (filteredRowIds.has(row.id) || (axis === "model" && anyHidden)) {
            return <FilteredValue />;
          }
          const [median] = performanceScore(row, sort, mode);
          return median < 0 ? "—" : formatPerformanceValue(median, sort);
        }}
        onSelect={onSelect}
      />
    </Panel>
  );
}
