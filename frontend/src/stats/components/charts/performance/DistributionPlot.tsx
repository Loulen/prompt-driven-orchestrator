import type { StatsDistribution } from "../../../../shared/generated/StatsDistribution";
import type { StatsSteeredRate } from "../../../../shared/generated/StatsSteeredRate";
import { harnessColor } from "../../../../harness/model/color";
import { Tooltip } from "../../../../shared/ui/tooltip";
import type { StatsZoom } from "../../../model/statsFilters";
import {
  distributionDetail,
  fieldMetric,
  formatPerformanceValue,
  steeredPercent,
  ZOOM_HIGH,
  ZOOM_LOW,
  type PerformanceField,
} from "../../../model/performance";

export function DistributionPlot({
  name,
  harness,
  field,
  value,
  scaleMax,
  zoom,
  rowMax = null,
  steered,
  waitMillis,
  ghost,
}: {
  name: string;
  harness: string;
  field: PerformanceField;
  value: StatsDistribution;
  scaleMax: number;
  /** What the whiskers reach and where the axis tops out (#811). */
  zoom: StatsZoom;
  /** The row's own cap, printed under the plot when « independent scales » is
   *  on — without it, two rows drawn full width read as comparable when they
   *  are not. `null` on the shared axis, where the header says the cap once. */
  rowMax?: number | null;
  /** The row × harness steered rate — read for the Steering metric only, where
   *  the line under the plot adds « · 23 % steered » (#792). */
  steered?: StatsSteeredRate;
  /** How much declared wait this row × harness lost to the active reading
   *  (#810), in milliseconds — the amber « −1m02s wait » that names what moved.
   *  Absent when the toggle is off or nothing was waited. */
  waitMillis?: number;
  /** The wall-clock Q1–Q3 the active box replaced (#810), drawn behind it as a
   *  dashed ghost: the reading the toggle hid stays reachable at a glance
   *  instead of vanishing. */
  ghost?: { q1: number; q3: number } | null;
}) {
  const metric = fieldMetric(field);
  if (!value.stats) {
    const detail = distributionDetail(name, harness, field, value, zoom);
    return (
      <Tooltip content={detail} side="top">
        <button
          type="button"
          aria-label={detail}
          className="min-w-0 truncate text-left text-text-3 underline decoration-dotted underline-offset-2"
        >
          — {value.missing_reasons[0] ?? "not measurable"}
        </button>
      </Tooltip>
    );
  }
  const stats = value.stats;
  const at = (raw: number) => Math.max(0, Math.min(100, (raw / scaleMax) * 100));
  const pct = (raw: number) => `${at(raw)}%`;
  // A span from `from` to `to`, cut at the frame's edge like its ends: a box
  // past the axis max never runs into the next column.
  const extent = (from: number, to: number, floor = 0) => ({
    left: `${Math.min(at(from), 100 - floor)}%`,
    width: `${Math.max(at(to) - at(from), floor)}%`,
  });
  const detail = distributionDetail(name, harness, field, value, zoom);
  const partial = value.measured < value.expected;
  const steeredLine = metric === "steering" ? steeredPercent(steered) : null;
  const low = stats[ZOOM_LOW[zoom]];
  const high = stats[ZOOM_HIGH[zoom]];
  // A clip mark says « something exists beyond the frame » without drawing it.
  // Fenced marks both tails (an outlier on either side is news); Box marks only
  // the long tail — below Q1 half the sample is always out, which is the level's
  // whole point, not an event.
  const clippedHigh = zoom !== "full" && stats.max > high;
  const clippedLow = zoom === "fenced" && stats.min < low;
  return (
    <div className="flex min-w-0 flex-1 flex-col gap-1">
      <div
        className="relative h-4"
        data-testid={`performance-${metric}-boxplot`}
        data-scale-max={scaleMax}
        data-zoom={zoom}
        aria-hidden="true"
      >
        {ghost && (
          <span
            className="absolute top-[4px] h-[7px] border border-dashed border-text-3 opacity-40"
            data-testid="performance-wallclock-ghost"
            style={extent(ghost.q1, ghost.q3, 0.5)}
          />
        )}
        {zoom !== "box" && (
          <span
            className="absolute top-[7px] h-px bg-text-3"
            data-testid={`performance-${metric}-whisker`}
            style={extent(low, high)}
          />
        )}
        <span
          className="absolute top-[4px] h-[7px] border border-current opacity-70"
          style={{
            color: harnessColor(harness),
            ...extent(stats.q1, stats.q3, 0.5),
          }}
        />
        <span
          className="absolute top-[3px] h-[9px] w-px bg-text"
          style={{ left: pct(stats.median) }}
        />
        {clippedLow && (
          <span
            className="absolute left-0 top-0 font-mono leading-4 text-text-3 text-2xs"
            data-testid={`performance-${metric}-clip-low`}
          >
            ‹
          </span>
        )}
        {clippedHigh && (
          <span
            className="absolute right-0 top-0 font-mono leading-4 text-text-3 text-2xs"
            data-testid={`performance-${metric}-clip-high`}
          >
            ›
          </span>
        )}
      </div>
      <Tooltip content={detail} side="top">
        <button
          type="button"
          aria-label={detail}
          className="max-w-full truncate text-left font-mono text-text-3 underline decoration-dotted underline-offset-2 text-2xs"
        >
          {formatPerformanceValue(stats.median, metric)} median · n=
          {value.measured}
          {partial ? " ⚠" : ""}
          {steeredLine ? ` · ${steeredLine} steered` : ""}
          {waitMillis ? (
            <span
              className="text-st-await"
              data-testid="performance-wait-delta"
            >
              {" "}
              −{formatPerformanceValue(waitMillis, "duration")} wait
            </span>
          ) : null}
          {rowMax !== null && (
            <span className="text-text-3">
              {" "}
              · ⤒ {formatPerformanceValue(rowMax, metric)}
            </span>
          )}
        </button>
      </Tooltip>
    </div>
  );
}
