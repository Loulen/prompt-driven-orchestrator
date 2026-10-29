import type { StatsHarnessPerformance } from "../../../../shared/generated/StatsHarnessPerformance";
import { harnessColor } from "../../../../harness/model/color";
import type { DurationMode, StatsZoom } from "../../../model/statsFilters";
import { metricField, type PerformanceMetric } from "../../../model/performance";
import { DistributionPlot } from "./DistributionPlot";

/** One metric cell of a Performance row: a box-plot per harness, on the axis
 *  the table hands down (shared per metric, or the row's own). */
export function MetricCell({
  name,
  rowHarnesses,
  metric,
  scales,
  harnesses,
  mode,
  zoom,
  independentAxis,
}: {
  name: string;
  rowHarnesses: StatsHarnessPerformance[] | undefined;
  metric: PerformanceMetric;
  scales: Record<PerformanceMetric, number>;
  harnesses: string[];
  mode: DurationMode;
  zoom: StatsZoom;
  independentAxis: boolean;
}) {
  const field = metricField(metric, mode);
  return (
    <td>
      <div className="grid grid-cols-[minmax(0,1fr)] gap-1.5">
        {harnesses.map((harness) => {
          const item = rowHarnesses?.find(
            (entry) => entry.harness === harness,
          );
          // What the active reading took off this row × harness, on the value
          // the row displays. Shown only when it is not zero: a node nobody
          // ever waited on carries no annotation. Waiting mode shows no
          // delta — the wait IS the value there, not something removed.
          const wait =
            field === "active_duration" &&
            item?.duration.stats &&
            item.active_duration.stats
              ? item.duration.stats.median - item.active_duration.stats.median
              : 0;
          return (
            <div key={harness} className="flex min-w-0 items-start gap-2">
              <span
                className="mt-1 h-[7px] w-[7px] shrink-0 rounded-full"
                style={{ backgroundColor: harnessColor(harness) }}
              />
              <DistributionPlot
                name={name}
                harness={harness}
                field={field}
                value={
                  item?.[field] ?? {
                    stats: null,
                    measured: 0,
                    expected: 0,
                    missing_reasons: [`never ran on ${harness}`],
                  }
                }
                scaleMax={scales[metric]}
                zoom={zoom}
                rowMax={independentAxis ? scales[metric] : null}
                steered={item?.steered}
                waitMillis={wait > 0 ? wait : undefined}
                ghost={wait > 0 ? (item?.duration.stats ?? null) : null}
              />
            </div>
          );
        })}
      </div>
    </td>
  );
}
