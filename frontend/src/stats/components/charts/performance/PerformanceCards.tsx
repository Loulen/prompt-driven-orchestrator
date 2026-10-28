import { Info } from "lucide-react";
import type { StatsDistribution } from "../../../../shared/generated/StatsDistribution";
import type { StatsPerformanceAggregate } from "../../../../shared/generated/StatsPerformanceAggregate";
import { harnessColor } from "../../../../harness/model/color";
import { Tooltip } from "../../../../shared/ui/tooltip";
import type { DurationMode } from "../../../model/statsFilters";
import {
  DECLARED_WAIT_COPY,
  durationWord,
  formatPerformanceValue,
  metricField,
  steeredPercent,
  STEERING_PROVENANCE_COPY,
  type PerformanceMetric,
} from "../../../model/performance";
import { FilteredValue } from "./NodeKindMarks";

export function PerformanceCards({
  aggregate,
  mode,
  partialByKind,
}: {
  aggregate: StatsPerformanceAggregate;
  mode: DurationMode;
  /** The kind filter hides part of what this aggregate pooled (#810). The head
   *  cards then fall back to « — »: they would otherwise show a number for a
   *  population that is no longer on screen, and a median of medians is not a
   *  median. */
  partialByKind: boolean;
}) {
  const durationField = metricField("duration", mode);
  const central = (value: StatsDistribution, metric: PerformanceMetric) => {
    if (partialByKind || !value.stats) return "—";
    return formatPerformanceValue(value.stats.median, metric);
  };
  return (
    <div className="grid gap-2 sm:grid-cols-2 xl:grid-cols-4">
      {aggregate.harnesses.map((item) => (
        <div
          key={item.harness}
          className="rounded-md border border-line bg-surface-2 p-3"
          data-testid={`stats-performance-card-${item.harness}`}
        >
          <div className="mb-2 flex items-center gap-1.5 text-text-3">
            <span
              className="h-2 w-2 rounded-full"
              style={{ backgroundColor: harnessColor(item.harness) }}
            />
            {item.harness}
          </div>
          <div className="font-mono text-text">
            {central(item.context, "context")} median context
          </div>
          <div className="flex items-center gap-1 font-mono text-text-3">
            <span>
              {central(item[durationField], "duration")} median{" "}
              {durationWord(mode)}
            </span>
            {mode !== "total" && (
              <Tooltip content={DECLARED_WAIT_COPY} side="top">
                <span
                  role="img"
                  aria-label={DECLARED_WAIT_COPY}
                  className="inline-flex text-text-3"
                >
                  <Info size={11} />
                </span>
              </Tooltip>
            )}
          </div>
          <div className="font-mono text-text-3">
            {central(item.steering, "steering")} median steering
          </div>
        </div>
      ))}
      <SteeredCard aggregate={aggregate} partialByKind={partialByKind} />
    </div>
  );
}

/** « Steered executions » (#792): one row per harness — the share of successful
 *  executions with ≥ 1 steering message, its coverage « steered / readable »
 *  and a thin bar; « — » with the reason when no count was readable. Follows
 *  the drill like the other cards (it reads the same aggregate). */
export function SteeredCard({
  aggregate,
  partialByKind = false,
}: {
  aggregate: StatsPerformanceAggregate;
  partialByKind?: boolean;
}) {
  return (
    <div
      className="rounded-md border border-line bg-surface-2 p-3 sm:col-span-2"
      data-testid="stats-steered-card"
    >
      <div className="mb-2 flex items-center justify-between gap-2 text-text-3">
        <span>Steered executions</span>
        <>
          <Tooltip content={STEERING_PROVENANCE_COPY} side="top">
            <span
              role="img"
              aria-label={STEERING_PROVENANCE_COPY}
              className="inline-flex text-text-3"
            >
              <Info size={12} />
            </span>
          </Tooltip>
        </>
      </div>
      <div className="grid gap-1.5">
        {aggregate.harnesses.map((item) => {
          // #810: a partial kind filter hides part of the executions this rate
          // was computed over — « filtered », never a share of a population the
          // user cannot see.
          const percent = partialByKind ? null : steeredPercent(item.steered);
          const ratio = item.steered.readable
            ? (item.steered.steered / item.steered.readable) * 100
            : 0;
          return (
            <div
              key={item.harness}
              className="flex items-center gap-2 font-mono"
              data-testid={`stats-steered-${item.harness}`}
            >
              <span
                className="h-2 w-2 shrink-0 rounded-full"
                style={{ backgroundColor: harnessColor(item.harness) }}
              />
              <span className="w-16 shrink-0 text-text-2">{item.harness}</span>
              {percent ? (
                <>
                  <span className="w-12 shrink-0 text-text">{percent}</span>
                  <span className="shrink-0 text-text-3">
                    · {item.steered.steered}/{item.steered.readable}
                  </span>
                  <span className="relative h-1 min-w-10 flex-1 rounded bg-surface-1">
                    <span
                      className="absolute inset-y-0 left-0 rounded"
                      style={{
                        width: `${ratio}%`,
                        backgroundColor: harnessColor(item.harness),
                      }}
                    />
                  </span>
                </>
              ) : partialByKind ? (
                <FilteredValue />
              ) : (
                <span className="text-text-3">
                  —{" "}
                  {item.steering.missing_reasons[0] ?? "no readable execution"}
                </span>
              )}
            </div>
          );
        })}
      </div>
      <div className="mt-2 text-text-3" style={{ fontSize: "10px" }}>
        share of successful executions with ≥ 1 steering message · steered /
        readable
      </div>
    </div>
  );
}
