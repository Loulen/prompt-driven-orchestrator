import type { StatsPerformanceAggregate } from "../../../../shared/generated/StatsPerformanceAggregate";
import type {
  DurationMode,
  StatsAxis,
  StatsZoom,
} from "../../../model/statsFilters";
import {
  durationWord,
  formatPerformanceValue,
  metricField,
  steeredPercent,
  ZOOM_CAP_COPY,
} from "../../../model/performance";
import { CohortLine, Kpis } from "../common/ChartPrimitives";
import { FilteredValue } from "./NodeKindMarks";
import { PerformanceCards } from "./PerformanceCards";

/** What the drill level on screen measured: the headline on one line, one
 *  card per harness, the cohort line. */
export function PerformanceSummary({
  aggregate,
  mode,
  partialByKind,
  completedOnly,
}: {
  aggregate: StatsPerformanceAggregate;
  mode: DurationMode;
  partialByKind: boolean;
  completedOnly: boolean;
}) {
  const durationField = metricField("duration", mode);
  const contexts = aggregate.harnesses.map((item) =>
    item.context.stats
      ? formatPerformanceValue(item.context.stats.median, "context")
      : "—",
  );
  const durations = aggregate.harnesses.map((item) =>
    item[durationField].stats
      ? formatPerformanceValue(item[durationField].stats!.median, "duration")
      : "—",
  );
  const steered = aggregate.harnesses.map(
    (item) => steeredPercent(item.steered) ?? "—",
  );
  const headlineValue = (values: string[]) =>
    partialByKind ? <FilteredValue /> : <>{values.join(" / ") || "—"}</>;

  return (
    <>
      <Kpis
        testid="stats-performance-headline"
        items={[
          { key: "context", label: "", value: headlineValue(contexts), sub: "median peak context" },
          { key: "duration", label: "", value: headlineValue(durations), sub: `median ${durationWord(mode)}` },
          { key: "steered", label: "", value: headlineValue(steered), sub: "steered" },
        ]}
      />
      <PerformanceCards
        aggregate={aggregate}
        mode={mode}
        partialByKind={partialByKind}
      />
      <CohortLine completedOnly={completedOnly} />
    </>
  );
}

/** What the plots below draw (#819): the caption of the table's panel. */
export function PerformanceCaption({ axisMode, zoom }: { axisMode: StatsAxis; zoom: StatsZoom }) {
  return (
    <span className="sx-note" data-testid="stats-performance-toolbar">
      Distributions per node ·{" "}
      {axisMode === "independent"
        ? "each row on its own axis"
        : `shared axis per metric, capped at ${ZOOM_CAP_COPY[zoom]}`}
    </span>
  );
}
