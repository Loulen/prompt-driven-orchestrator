import { Info } from "lucide-react";
import { Tooltip } from "../../../../shared/ui/tooltip";
import type { DurationMode } from "../../../model/statsFilters";
import {
  DECLARED_WAIT_COPY,
  MODE_FIELD,
  PERFORMANCE_FIELD_LABEL,
  STEERING_PROVENANCE_COPY,
} from "../../../model/performance";

/** The Performance table's header: the Duration column names the reading the
 *  duration mode picked (#819), Steering says where it comes from (#792). */
export function PerformanceTableHead({ mode }: { mode: DurationMode }) {
  return (
    <thead className="text-text-3">
      <tr>
        <th className="w-48 pb-2 font-medium">Name</th>
        <th className="pb-2 font-medium">Context (peak tokens)</th>
        <th
          className="pb-2 font-medium"
          data-testid="stats-performance-duration-header"
        >
          {mode === "total" ? (
            "Duration"
          ) : (
            <span className="inline-flex items-center gap-1">
              {PERFORMANCE_FIELD_LABEL[MODE_FIELD[mode]]}
              <Tooltip content={DECLARED_WAIT_COPY} side="top">
                <span
                  role="img"
                  aria-label={DECLARED_WAIT_COPY}
                  className="inline-flex text-text-3"
                >
                  <Info size={11} />
                </span>
              </Tooltip>
            </span>
          )}
        </th>
        <th className="pb-2 font-medium">
          <span className="inline-flex items-center gap-1">
            Steering (messages / execution)
            <Tooltip content={STEERING_PROVENANCE_COPY} side="top">
              <span
                role="img"
                aria-label={STEERING_PROVENANCE_COPY}
                className="inline-flex text-text-3"
              >
                <Info size={11} />
              </span>
            </Tooltip>
          </span>
        </th>
      </tr>
    </thead>
  );
}
