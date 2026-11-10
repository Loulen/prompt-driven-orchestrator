import type { StatsPerformance } from "../../../../shared/generated/StatsPerformance";
import {
  performanceBandDeviates,
  type DurationMode,
  type NodeKind,
  type PerformanceBand,
  type StatsAxis,
  type StatsZoom,
} from "../../../model/statsFilters";
import {
  BandDivider,
  CohortChip,
  StatsFilterBand,
  UncombinedChip,
} from "../filters/FilterBand";
import {
  DurationModeSegments,
  NodeKindChips,
  WaitCoverage,
} from "../filters/PerformanceFilterControls";
import { IndependentScalesSwitch, ZoomSegments } from "./ZoomControls";

/** The Performance band, read left → right in the order the numbers depend on
 *  it: which Runs, which duration (and how many executions qualify), which
 *  Nodes, how the plots are drawn. */
export function PerformanceFilterBand({
  performance,
  kindCounts,
  completedOnly,
  onCompletedOnlyChange,
  uncombined,
  onUncombinedChange,
  showUncombined,
  band,
  onBandChange,
  onResetFilters,
  crumbs,
}: {
  /** The drill path, on the left of the bar. */
  crumbs?: React.ReactNode;
  performance: StatsPerformance;
  kindCounts: Record<NodeKind, number>;
  completedOnly: boolean;
  onCompletedOnlyChange: (value: boolean) => void;
  uncombined: boolean;
  onUncombinedChange: (value: boolean) => void;
  showUncombined: boolean;
  band: PerformanceBand;
  onBandChange: (band: PerformanceBand) => void;
  onResetFilters: () => void;
}) {
  const { durationMode: mode, nodeKinds, zoom, axis: axisMode } = band;
  const changeZoom = (next: StatsZoom) => onBandChange({ ...band, zoom: next });
  const changeAxisMode = (next: StatsAxis) =>
    onBandChange({ ...band, axis: next });
  const onDurationModeChange = (next: DurationMode) =>
    onBandChange({ ...band, durationMode: next });
  const onNodeKindsChange = (kinds: NodeKind[]) =>
    onBandChange({ ...band, nodeKinds: kinds });
  return (
    <StatsFilterBand
      label="Performance filters"
      testid="stats-performance-filters"
      crumbs={crumbs}
      onReset={
        performanceBandDeviates(completedOnly, band)
          ? onResetFilters
          : undefined
      }
    >
      <CohortChip checked={completedOnly} onChange={onCompletedOnlyChange} />
      {showUncombined && (
        <UncombinedChip checked={uncombined} onChange={onUncombinedChange} />
      )}
      <BandDivider />
      <DurationModeSegments value={mode} onChange={onDurationModeChange} />
      {mode !== "total" && (
        <WaitCoverage
          waited={performance.waited_executions ?? 0}
          executions={performance.executions ?? 0}
        />
      )}
      <BandDivider />
      <NodeKindChips
        counts={kindCounts}
        nodeKinds={nodeKinds}
        onNodeKindsChange={onNodeKindsChange}
      />
      <BandDivider />
      <ZoomSegments value={zoom} onChange={changeZoom} />
      <IndependentScalesSwitch
        checked={axisMode === "independent"}
        onChange={(checked) =>
          changeAxisMode(checked ? "independent" : "shared")
        }
      />
    </StatsFilterBand>
  );
}
