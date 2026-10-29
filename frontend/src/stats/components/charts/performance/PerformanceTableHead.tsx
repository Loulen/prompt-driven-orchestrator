import { Info } from "lucide-react";
import { Tooltip } from "../../../../shared/ui/tooltip";
import type { DurationMode } from "../../../model/statsFilters";
import type { PerformanceMetric } from "../../../model/performance";
import { SortTh } from "../common/SortHeader";
import {
  DECLARED_WAIT_COPY,
  MODE_FIELD,
  PERFORMANCE_FIELD_LABEL,
  STEERING_PROVENANCE_COPY,
} from "../../../model/performance";

/** The Performance table's header: the Duration column names the reading the
 *  duration mode picked (#819), Steering says where it comes from (#792). */
export function PerformanceTableHead({
  mode,
  sort,
  onSortChange,
}: {
  mode: DurationMode;
  sort: PerformanceMetric;
  onSortChange?: (sort: PerformanceMetric) => void;
}) {
  // The metric columns sort the rows, always the larger first (the ranking's
  // order); the Name column does not.
  const tableSort = { key: sort, dir: "desc" as const };
  const onSort = (key: string) => onSortChange?.(key as PerformanceMetric);
  return (
    <thead>
      <tr>
        <th>Name</th>
        <SortTh sort={tableSort} onSort={onSort} sortKey="context">
          Context (peak tokens)
        </SortTh>
        <SortTh
          sort={tableSort}
          onSort={onSort}
          sortKey="duration"
          testid="stats-performance-duration-header"
          after={mode === "total" ? null : <InfoMark copy={DECLARED_WAIT_COPY} />}
        >
          {mode === "total" ? "Duration" : PERFORMANCE_FIELD_LABEL[MODE_FIELD[mode]]}
        </SortTh>
        <SortTh
          sort={tableSort}
          onSort={onSort}
          sortKey="steering"
          after={<InfoMark copy={STEERING_PROVENANCE_COPY} />}
        >
          Steering (messages / execution)
        </SortTh>
      </tr>
    </thead>
  );
}

/** An « i » after a header's sort button: its tooltip says where the figures come from. */
function InfoMark({ copy }: { copy: string }) {
  return (
    <Tooltip content={copy} side="top">
      <span role="img" aria-label={copy} className="ml-1 inline-flex align-middle text-text-3">
        <Info size={11} />
      </span>
    </Tooltip>
  );
}
