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
import { MasterList } from "../common/MasterList";
import { FilteredValue } from "./NodeKindMarks";

/** The Performance ranking: the grouping and sort selects (#737) over the
 *  master list. */
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
  return (
    <aside className="w-[290px] shrink-0 border-r border-line pr-4">
      <div className="mb-2 flex items-center justify-between gap-2">
        <span className="text-fg-4" style={{ fontSize: "10.5px" }}>
          Ranked by {performanceSortLabel(sort, mode)} (median)
        </span>
        <span className="flex items-center gap-1.5">
          <select
            aria-label="Performance grouping"
            value={axis}
            onChange={(event) =>
              onAxisChange(event.target.value as PerformanceAxis)
            }
            className="rounded border border-line bg-bg-3 px-2 py-1 text-fg-2"
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
            className="rounded border border-line bg-bg-3 px-2 py-1 text-fg-2"
          >
            <option value="context">By context</option>
            <option value="duration">By {durationWord(mode)}</option>
            <option value="steering">By steering</option>
          </select>
        </span>
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
      {axis === "model" && (
        <div className="mt-3 text-fg-4" style={{ fontSize: "10.5px" }}>
          Model ids verbatim, one row per id — the same id run through two
          harnesses is one row, one column per harness. Hover a model or an
          effort for where the value was read.
        </div>
      )}
    </aside>
  );
}
