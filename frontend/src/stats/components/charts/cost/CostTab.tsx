import { useState } from "react";
import type { StatsCost } from "../../../../shared/generated/StatsCost";
import type { StatsCostEntity } from "../../../../shared/generated/StatsCostEntity";
import type { StatsEffortCostEntity } from "../../../../shared/generated/StatsEffortCostEntity";
import type { StatsProjectCostEntity } from "../../../../shared/generated/StatsProjectCostEntity";
import { formatCostAmount } from "../../../../run/model/costLabel";
import { useStatsAbsorption } from "../../../store/useStatsAbsorption";
import {
  coupleRows,
  coupleScope,
  hasExplicitEffort,
  type CoupleRow,
} from "../../../model/statsAbsorption";
import { isCostNodeRow, type CostAxis } from "../../../model/cost";
import { RUNS_COUNT } from "../../../model/rowCounts";
import { Breadcrumb } from "../common/Breadcrumb";
import { Info } from "lucide-react";
import { Tooltip } from "../../../../shared/ui/tooltip";
import type { TabFrame } from "../../StatsCharts";
import { EmptyNote, Panel } from "../common/ChartPrimitives";
import { MasterList } from "../common/MasterList";
import { ProvenanceName } from "../common/Provenance";
import {
  PipelineName,
  TableCombinedIcon,
} from "../common/TableCombinedIcon";
import type { CoupleSelection } from "../common/tableSelection";
import { effortNameCell } from "./effortNameCell";
import { CostSummary } from "./CostSummary";
import { CostTable } from "./CostTable";

export function CostTab({
  cost,
  error,
  uncombined,
  onAbsorptionsChanged,
  frame,
}: {
  cost: StatsCost | null;
  error: string | null;
  uncombined: boolean;
  onAbsorptionsChanged: () => void;
  frame: TabFrame;
}) {
  const [axis, setAxis] = useState<CostAxis>("pipeline");
  // #890 / #892: Pipelines on « By pipeline », Models on « By model », the Nodes
  // of the Pipeline on screen on every axis — never the Projects. #906: the
  // efforts of one model, and the couples of one Node. Switching the grouping
  // drops the selection with its rows.
  const absorption = useStatsAbsorption({
    enabled: !uncombined,
    onChanged: onAbsorptionsChanged,
    onOpenModelAxis: (row) => {
      setAxis("model");
      toTotal();
      setSelectedModelId((row as CoupleRow).model ?? null);
    },
  });
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [drilledPipelineId, setDrilledPipelineId] = useState<string | null>(
    null,
  );
  // The model axis drills Model → Effort → Pipeline → Node (ADR-0065). The
  // effort id is "" for the "not set" bucket, so selection is `null` vs value,
  // never falsy-compared.
  const [selectedModelId, setSelectedModelId] = useState<string | null>(null);
  const [selectedEffortId, setSelectedEffortId] = useState<string | null>(null);
  const [selectedPipelineId, setSelectedPipelineId] = useState<string | null>(
    null,
  );

  if (error || !cost) {
    return (
      <div className="sx-tab" data-testid="stats-chart-cost">
        {frame.band()}
        <div className="sx-page">
          {error ? (
            <div className="sx-err" role="alert">
              {error}
            </div>
          ) : (
            <EmptyNote>Loading cost…</EmptyNote>
          )}
        </div>
      </div>
    );
  }

  const toTotal = () => {
    setSelectedId(null);
    setDrilledPipelineId(null);
    setSelectedModelId(null);
    setSelectedEffortId(null);
    setSelectedPipelineId(null);
  };

  // Model axis: resolve the drill path.
  const model =
    axis === "model"
      ? (cost.by_model.find((row) => row.id === selectedModelId) ?? null)
      : null;
  const effort =
    model && selectedEffortId !== null
      ? (model.efforts.find((row) => row.id === selectedEffortId) ?? null)
      : null;
  const modelPipeline =
    effort && selectedPipelineId !== null
      ? (effort.pipelines.find((row) => row.id === selectedPipelineId) ?? null)
      : null;

  const rows: StatsCostEntity[] =
    axis === "pipeline"
      ? cost.by_pipeline
      : axis === "project"
        ? cost.by_project
        : cost.by_model;
  const selected =
    axis === "model"
      ? null
      : (rows.find((row) => row.id === selectedId) ?? null);
  const drilledPipeline =
    axis === "project" && selected
      ? ((selected as StatsProjectCostEntity).pipelines.find(
          (pipeline) => pipeline.id === drilledPipelineId,
        ) ?? null)
      : null;

  const aggregate =
    axis === "model"
      ? (modelPipeline ?? effort ?? model ?? cost.total)
      : (drilledPipeline ?? selected ?? cost.total);
  const periods =
    axis === "model"
      ? ((modelPipeline ?? effort ?? model)?.by_period ?? cost.by_period)
      : drilledPipeline
        ? drilledPipeline.by_period
        : selected
          ? selected.by_period
          : cost.by_period;

  // The denominator the headline names (ADR-0065 §3): a model bucket counts
  // executions — the whole model axis reads "per execution" — and so does the
  // Node level of the other axes.
  const atNodeLevel =
    axis === "model"
      ? true
      : axis === "pipeline"
        ? selected !== null
        : drilledPipeline !== null;
  const detailUnit: "Run" | "execution" = atNodeLevel ? "execution" : "Run";

  const masterSelection =
    axis === "pipeline"
      ? absorption.selectionFor({
          dimension: "pipeline",
          rows: cost.by_pipeline,
          count: RUNS_COUNT,
        })
      : axis === "model"
        ? absorption.selectionFor({
            dimension: "model",
            rows: cost.by_model,
            count: RUNS_COUNT,
          })
        : undefined;
  // The Pipeline whose Nodes the table shows, on whichever axis it was reached.
  const nodePipeline =
    axis === "model"
      ? modelPipeline
      : axis === "project"
        ? drilledPipeline
        : selected;
  const nodeSelection = nodePipeline
    ? absorption.selectionFor({
        dimension: "node",
        scope: { key: nodePipeline.id, name: nodePipeline.name },
        rows: nodePipeline.nodes,
        count: RUNS_COUNT,
        selectable: isCostNodeRow,
      })
    : undefined;
  // #906: the efforts of the model on screen, and the couples of each Node.
  const effortSelection =
    model && !effort
      ? absorption.selectionFor({
          dimension: "effort",
          scope: { key: model.id, name: model.name },
          rows: model.efforts,
          count: RUNS_COUNT,
          preferred: hasExplicitEffort,
        })
      : undefined;
  const coupleSelection: CoupleSelection | undefined = nodePipeline
    ? (node) =>
        absorption.selectionFor({
          dimension: "couple",
          scope: {
            key: coupleScope(nodePipeline.id, node.id),
            name: `${node.name} (${nodePipeline.name})`,
          },
          rows: coupleRows(
            nodePipeline.nodes.find((item) => item.id === node.id)?.models ??
              [],
          ),
          count: RUNS_COUNT,
          // Infrastructure and Unassigned are no Node: their couples show the
          // global mark, never a ring.
          selectable: () => isCostNodeRow(node as StatsCostEntity),
          preferred: hasExplicitEffort,
        })
    : undefined;

  let detailRows: StatsCostEntity[];
  let detailRenderName: ((row: StatsCostEntity) => React.ReactNode) | undefined;
  let onOpen: ((row: StatsCostEntity) => void) | undefined;
  if (axis === "model") {
    if (modelPipeline) {
      detailRows = modelPipeline.nodes;
    } else if (effort) {
      detailRows = effort.pipelines;
      onOpen = (row) => setSelectedPipelineId(row.id);
    } else if (model) {
      detailRows = model.efforts;
      detailRenderName = (row) => effortNameCell(row as StatsEffortCostEntity);
      onOpen = (row) => setSelectedEffortId(row.id);
    } else {
      detailRows = cost.by_model;
      detailRenderName = (row) => (
        <span className="inline-flex items-center gap-1.5">
          <ProvenanceName
            name={row.name}
            mono
            provenance={cost.by_model.find((m) => m.id === row.id)?.provenance}
            harnesses={row.harnesses}
            target="model"
          />
          <TableCombinedIcon selection={masterSelection} row={row} />
        </span>
      );
      onOpen = (row) => setSelectedModelId(row.id);
    }
  } else if (drilledPipeline) {
    detailRows = drilledPipeline.nodes;
  } else if (!selected) {
    detailRows = rows;
    if (axis === "pipeline") {
      detailRenderName = (row) => (
        <PipelineName
          row={row}
          onOpenMembers={masterSelection?.onOpenMembers}
        />
      );
    }
  } else if (axis === "project") {
    detailRows = (selected as StatsProjectCostEntity).pipelines;
    onOpen = (pipeline) => setDrilledPipelineId(pipeline.id);
  } else {
    detailRows = selected.nodes;
  }

  // #736: a harness without a cost source (opencode) has no model row,
  // so the model axis drops its column; the other axes keep it and show « — ».
  // The daemon's reason string is "harness has no cost source" — match on the
  // stable substring, not the full wire value.
  const noCostSource = cost.harnesses.filter((h) =>
    cost.total.harnesses
      .find((m) => m.harness === h)
      ?.missing_reasons.some((r) => r.includes("no cost source")),
  );
  const tableHarnesses =
    axis === "model"
      ? cost.harnesses.filter((h) => !noCostSource.includes(h))
      : cost.harnesses;

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
  } else {
    if (selected)
      crumbs.push({
        label: selected.name,
        onClick: () => setDrilledPipelineId(null),
      });
    if (drilledPipeline) crumbs.push({ label: drilledPipeline.name });
  }
  // Every crumb but the last pops the levels it shadows (design: clickable
  // breadcrumb for all three axes once there are four levels).
  const clickableCrumbs = crumbs.map((crumb, index) =>
    index === crumbs.length - 1 ? { label: crumb.label } : crumb,
  );

  const axisNote = [
    "Model ids verbatim, one row per id — the same id run through two harnesses is one row, one column per harness. Hover a model or an effort for where the value was read and its provider.",
    noCostSource.length > 0
      ? `${noCostSource.join(", ")} has no cost source and is not on this axis.`
      : "",
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <div className="sx-tab" data-testid="stats-chart-cost">
      {frame.band(<Breadcrumb crumbs={clickableCrumbs} />)}
      <div className="sx-page">
        {frame.cohort}
        <CostSummary
          aggregate={aggregate}
          periods={periods}
          harnesses={cost.harnesses}
          unit={detailUnit}
        />
        <div className="sx-split">
          <Panel
            title="Ranked by cost"
            flush
            className="sx-aside"
            testid="stats-drilldown-navigation"
            aside={
              <>
                {axis === "model" && (
                  <Tooltip content={axisNote} side="top">
                    <span role="img" aria-label={axisNote} className="inline-flex text-text-3">
                      <Info size={12} strokeWidth={1.5} />
                    </span>
                  </Tooltip>
                )}
                <select
                  aria-label="Cost grouping"
                  value={axis}
                  onChange={(event) => {
                    setAxis(event.target.value as CostAxis);
                    toTotal();
                    absorption.clear();
                  }}
                  className="sx-select"
                >
                  <option value="pipeline">By pipeline</option>
                  <option value="project">By project</option>
                  <option value="model">By model</option>
                </select>
              </>
            }
          >
            <MasterList
              rows={rows}
              selected={axis === "model" ? selectedModelId : selectedId}
              monoName={axis === "model"}
              valueLabel={(row) =>
                formatCostAmount(row.usd, row.partial, row.estimated)
              }
              selection={masterSelection}
              onSelect={(id) => {
                if (axis === "model") {
                  setSelectedModelId(id);
                  setSelectedEffortId(null);
                  setSelectedPipelineId(null);
                } else {
                  setSelectedId(id);
                  setDrilledPipelineId(null);
                }
              }}
            />
          </Panel>

          <div className="sx-detail" data-testid="stats-drilldown-detail">
            <Panel
              title={
                crumbs.length > 1
                  ? crumbs[crumbs.length - 1].label
                  : { pipeline: "By pipeline", project: "By project", model: "By model" }[axis]
              }
              count={detailRows.length}
              flush
            >
              <CostTable
                key={`${axis}-${selectedId ?? ""}-${drilledPipelineId ?? ""}-${selectedModelId ?? ""}-${selectedEffortId ?? "total"}-${selectedPipelineId ?? ""}`}
                rows={detailRows}
                harnesses={tableHarnesses}
                unit={detailUnit}
                onOpen={onOpen}
                renderName={detailRenderName}
                expandablePairs={axis !== "model"}
                selection={
                  detailRows === nodePipeline?.nodes
                    ? nodeSelection
                    : detailRows === model?.efforts
                      ? effortSelection
                      : undefined
                }
                coupleSelection={
                  detailRows === nodePipeline?.nodes ? coupleSelection : undefined
                }
              />
            </Panel>
          </div>
        </div>
      </div>
      {absorption.overlay}
    </div>
  );
}
