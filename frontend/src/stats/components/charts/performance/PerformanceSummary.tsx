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
import { CohortLine } from "../common/ChartPrimitives";
import { FilteredValue } from "./NodeKindMarks";
import { PerformanceCards } from "./PerformanceCards";

/** What the drill level on screen measured: the headline, one card per
 *  harness, the cohort line and the caption of the plots below. */
export function PerformanceSummary({
  aggregate,
  mode,
  partialByKind,
  completedOnly,
  axisMode,
  zoom,
}: {
  aggregate: StatsPerformanceAggregate;
  mode: DurationMode;
  partialByKind: boolean;
  completedOnly: boolean;
  axisMode: StatsAxis;
  zoom: StatsZoom;
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
      <div className="mt-4 text-fg" data-testid="stats-performance-headline">
        {headlineValue(contexts)} median peak context ·{" "}
        {headlineValue(durations)} median {durationWord(mode)} ·{" "}
        {headlineValue(steered)} steered
      </div>
      <div className="mt-4">
        <PerformanceCards
          aggregate={aggregate}
          mode={mode}
          partialByKind={partialByKind}
        />
      </div>
      <div className="mt-4">
        <CohortLine completedOnly={completedOnly} />
      </div>
      {/* #819 — the zoom and the scales moved up into the band; what is
          left here is the caption that says what the plots below draw. */}
      <div
        className="mt-5 text-fg-4"
        data-testid="stats-performance-toolbar"
        style={{ fontSize: "10.5px" }}
      >
        Distributions per node ·{" "}
        {axisMode === "independent"
          ? "each row on its own axis"
          : `shared axis per metric, capped at ${ZOOM_CAP_COPY[zoom]}`}
      </div>
    </>
  );
}
