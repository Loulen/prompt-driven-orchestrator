import { useState } from "react";
import type { StatsPerformance } from "../../../../shared/generated/StatsPerformance";
import type { StatsPerformanceEntity } from "../../../../shared/generated/StatsPerformanceEntity";
import { useStatsAbsorption } from "../../../store/useStatsAbsorption";
import type { CoupleRow } from "../../../model/statsAbsorption";
import {
  ALL_NODE_KINDS,
  type NodeKind,
  type PerformanceBand,
} from "../../../model/statsFilters";
import {
  sortPerformance,
  type PerformanceAxis,
  type PerformanceMetric,
} from "../../../model/performance";
import {
  filteredRowIdsAt,
  hasHiddenNode,
  matchesKinds,
  nodeKindCounts,
  partialByKindAt,
} from "../../../model/nodeKind";
import { Breadcrumb } from "../common/Breadcrumb";
import { EmptyNote, HarnessLegend } from "../common/ChartPrimitives";
import { NoNodeOfSelectedKinds } from "./NodeKindMarks";
import { PerformanceAside } from "./PerformanceAside";
import {
  performanceDetail,
  performanceSelections,
  type PerformanceLevel,
} from "./performanceDetail";
import { PerformanceFilterBand } from "./PerformanceFilterBand";
import { PerformanceSummary } from "./PerformanceSummary";
import { PerformanceTable } from "./PerformanceTable";

export function PerformanceTab({
  performance,
  error,
  completedOnly,
  onCompletedOnlyChange,
  band,
  onBandChange,
  onResetFilters,
  uncombined,
  onUncombinedChange,
  showUncombined,
  onAbsorptionsChanged,
}: {
  performance: StatsPerformance | null;
  error: string | null;
  completedOnly: boolean;
  onCompletedOnlyChange: (value: boolean) => void;
  uncombined: boolean;
  onUncombinedChange: (value: boolean) => void;
  showUncombined: boolean;
  /** The band's controls, owned by the shell so a tab switch keeps them (#819). */
  band: PerformanceBand;
  onBandChange: (band: PerformanceBand) => void;
  onResetFilters: () => void;
  onAbsorptionsChanged: () => void;
}) {
  // The second select (#737): grouping (« By pipeline » / « By model »), fully
  // independent of the sort (« By context » / « By duration ») beside it.
  const [axis, setAxis] = useState<PerformanceAxis>("pipeline");
  const [sort, setSort] = useState<PerformanceMetric>("context");
  const [selectedId, setSelectedId] = useState<string | null>(null);
  // The model axis drills Model → Effort → Pipeline → Node (ADR-0065). The
  // effort id is "" for the "not set" bucket, so selection is `null` vs value,
  // never falsy-compared.
  const [selectedModelId, setSelectedModelId] = useState<string | null>(null);
  const [selectedEffortId, setSelectedEffortId] = useState<string | null>(null);
  const [selectedPipelineId, setSelectedPipelineId] = useState<string | null>(
    null,
  );
  // #819 — the band's controls come from the shell, which outlives the section:
  // they survive the drill, the grouping switch, the table's keyed remount AND
  // a trip to another tab, while a fresh Stats always opens on the defaults.
  const { durationMode: mode, nodeKinds, zoom, axis: axisMode } = band;
  const onNodeKindsChange = (kinds: NodeKind[]) =>
    onBandChange({ ...band, nodeKinds: kinds });
  // #890 / #892: the « By pipeline » Pipeline rows — never the Infrastructure
  // row beside them —, the « By model » Model rows, and the Nodes of the
  // Pipeline on screen.
  const absorption = useStatsAbsorption({
    enabled: !uncombined,
    onChanged: onAbsorptionsChanged,
    onOpenModelAxis: (row) => {
      setAxis("model");
      toTotal();
      setSelectedModelId((row as CoupleRow).model ?? null);
    },
  });

  if (error) {
    return (
      <div className="rounded-md border border-st-failed/30 bg-st-failed/14 px-3 py-2 text-st-failed">
        {error}
      </div>
    );
  }
  if (!performance) return <EmptyNote>Loading performance…</EmptyNote>;

  const toTotal = () => {
    setSelectedId(null);
    setSelectedModelId(null);
    setSelectedEffortId(null);
    setSelectedPipelineId(null);
  };

  // --- Node kind filter (#810) ------------------------------------------------
  const kindCounts = nodeKindCounts(performance.by_pipeline);
  const kindFilterActive = nodeKinds.length < ALL_NODE_KINDS.length;
  const anyHidden =
    kindFilterActive &&
    performance.by_pipeline.some((row) => hasHiddenNode(row, nodeKinds));
  const visibleNodes = (rows: StatsPerformanceEntity[]) =>
    rows.filter((row) => matchesKinds(row, nodeKinds));
  const filterStrip = (
    <PerformanceFilterBand
      performance={performance}
      kindCounts={kindCounts}
      completedOnly={completedOnly}
      onCompletedOnlyChange={onCompletedOnlyChange}
      uncombined={uncombined}
      onUncombinedChange={onUncombinedChange}
      showUncombined={showUncombined}
      band={band}
      onBandChange={onBandChange}
      onResetFilters={onResetFilters}
    />
  );

  // Every empty state keeps the band above it: the cohort now lives there, so a
  // pane emptied BY the cohort must still carry the one control that refills it
  // (a fresh instance with no completed run would otherwise be a dead end).
  const emptyPane = (note: React.ReactNode) => (
    <>
      <div data-testid="stats-chart-performance">
        {filterStrip}
        {note}
      </div>
    </>
  );

  if (nodeKinds.length === 0) {
    return emptyPane(
      <NoNodeOfSelectedKinds
        where="in this period"
        onShowAll={() => onNodeKindsChange([...ALL_NODE_KINDS])}
      />,
    );
  }
  if (
    performance.by_pipeline.length === 0 &&
    performance.infrastructure.length === 0 &&
    performance.by_model.length === 0
  ) {
    return emptyPane(
      <EmptyNote>No successful executions in this period.</EmptyNote>,
    );
  }
  // A period whose observations all lack a resolvable model is an empty axis —
  // its own absence, not a broken tab.
  if (axis === "model" && performance.by_model.length === 0) {
    return emptyPane(<EmptyNote>No model observed in this period.</EmptyNote>);
  }

  const infrastructureRow: StatsPerformanceEntity = {
    id: "__infrastructure__",
    name: "Infrastructure",
    ...performance.infrastructure_total,
    nodes: performance.infrastructure,
    subagents: [],
  };

  // Model axis: resolve the drill path.
  const model =
    axis === "model"
      ? (performance.by_model.find((row) => row.id === selectedModelId) ?? null)
      : null;
  const effort =
    model && selectedEffortId !== null
      ? (model.efforts.find((row) => row.id === selectedEffortId) ?? null)
      : null;
  const modelPipeline =
    effort && selectedPipelineId !== null
      ? (effort.pipelines.find((row) => row.id === selectedPipelineId) ?? null)
      : null;

  // A Pipeline whose every node is hidden leaves the master list; one that was
  // selected then falls back to Total, because `selected` is looked up in the
  // filtered rows.
  const pipelineRows = performance.by_pipeline.filter(
    (row) =>
      row.nodes.length === 0 ||
      row.nodes.some((node) => matchesKinds(node, nodeKinds)),
  );
  const masterRows =
    axis === "model"
      ? sortPerformance(performance.by_model, sort, mode)
      : sortPerformance([...pipelineRows, infrastructureRow], sort, mode);
  const selected =
    axis === "pipeline"
      ? (masterRows.find((row) => row.id === selectedId) ?? null)
      : null;
  const aggregate =
    axis === "model"
      ? (modelPipeline ?? effort ?? model ?? performance.total)
      : (selected ?? performance.total);
  const partialByKind = partialByKindAt(axis, selected, anyHidden, nodeKinds);

  const level: PerformanceLevel = {
    axis,
    model,
    effort,
    modelPipeline,
    selected,
  };
  const {
    masterSelection,
    nodePipeline,
    nodeSelection,
    effortSelection,
    coupleSelection,
  } = performanceSelections(absorption, performance, level);
  const { detailRows, unfilteredNodes, detailRenderName, onOpen } =
    performanceDetail({
      level,
      performance,
      masterRows,
      masterSelection,
      visibleNodes,
      onOpenModel: setSelectedModelId,
      onOpenEffort: setSelectedEffortId,
      onOpenPipeline: setSelectedPipelineId,
    });

  // A Node level the filter emptied: the table would stand there with its
  // headers and not a row, saying nothing about why. Same state, same way out
  // as every chip off.
  const detailEmptiedByKind =
    detailRows.length === 0 && (unfilteredNodes?.length ?? 0) > 0;
  const filteredRowIds = filteredRowIdsAt({
    axis,
    byPipeline: performance.by_pipeline,
    detailRows,
    kinds: nodeKinds,
    kindFilterActive,
    anyHidden,
  });

  // The model axis's breadcrumb; « By pipeline » keeps its one-line header.
  const crumbs: { label: string; onClick?: () => void }[] = [
    { label: "Total", onClick: toTotal },
  ];
  if (axis === "model") {
    if (model)
      crumbs.push({
        label: model.name,
        onClick: () => {
          setSelectedEffortId(null);
          setSelectedPipelineId(null);
        },
      });
    if (effort)
      crumbs.push({
        label: effort.name,
        onClick: () => setSelectedPipelineId(null),
      });
    if (modelPipeline) crumbs.push({ label: modelPipeline.name });
  }
  // Every crumb but the last pops the levels it shadows.
  const clickableCrumbs = crumbs.map((crumb, index) =>
    index === crumbs.length - 1 ? { label: crumb.label } : crumb,
  );

  return (
    // #810 — the band, the master-list « filtered » marks and the card « i » all
    // carry tooltips outside `PerformanceTable`'s own provider.
    <>
      <div
        className="relative flex min-h-full flex-col"
        data-testid="stats-chart-performance"
      >
        {/* #819 — the band spans the whole pane, above the ranking AND the
            detail: it governs both, and a control that governs the master list
            must not sit beside it. */}
        {filterStrip}
        <div className="flex min-h-0 flex-1">
          <PerformanceAside
            axis={axis}
            onAxisChange={(next) => {
              setAxis(next);
              toTotal();
              absorption.clear();
            }}
            sort={sort}
            onSortChange={setSort}
            mode={mode}
            masterRows={masterRows}
            masterSelection={masterSelection}
            selected={axis === "model" ? selectedModelId : selectedId}
            filteredRowIds={filteredRowIds}
            anyHidden={anyHidden}
            onSelect={(id) => {
              if (axis === "model") {
                setSelectedModelId(id);
                setSelectedEffortId(null);
                setSelectedPipelineId(null);
              } else {
                setSelectedId(id);
              }
            }}
          />
          <div className="min-w-0 flex-1 pl-5">
            {axis === "model" ? (
              <Breadcrumb
                testid="stats-performance-breadcrumb"
                crumbs={clickableCrumbs}
              />
            ) : (
              <div className="mb-3 text-text-3" style={{ fontSize: "10.5px" }}>
                Total{selected ? ` / ${selected.name}` : ""}
              </div>
            )}
            <HarnessLegend harnesses={performance.harnesses} />
            <PerformanceSummary
              aggregate={aggregate}
              mode={mode}
              partialByKind={partialByKind}
              completedOnly={completedOnly}
              axisMode={axisMode}
              zoom={zoom}
            />
            <div className="mt-2 min-h-[240px]">
              {detailEmptiedByKind ? (
                <NoNodeOfSelectedKinds
                  where="at this level"
                  onShowAll={() => onNodeKindsChange([...ALL_NODE_KINDS])}
                />
              ) : (
                <PerformanceTable
                  key={`${axis}-${selectedId ?? ""}-${selectedModelId ?? "total"}-${selectedEffortId ?? "total"}-${selectedPipelineId ?? ""}`}
                  rows={detailRows}
                  harnesses={performance.harnesses}
                  sort={sort}
                  mode={mode}
                  zoom={zoom}
                  independentAxis={axisMode === "independent"}
                  renderName={detailRenderName}
                  onOpen={onOpen}
                  expandablePairs={axis === "pipeline"}
                  filteredRowIds={filteredRowIds}
                  selection={
                    nodePipeline
                      ? nodeSelection
                      : detailRows === model?.efforts
                        ? effortSelection
                        : undefined
                  }
                  coupleSelection={nodePipeline ? coupleSelection : undefined}
                />
              )}
            </div>
          </div>
        </div>
        {absorption.overlay}
      </div>
    </>
  );
}
