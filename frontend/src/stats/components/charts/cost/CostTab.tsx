import { useState } from "react";
import type { StatsCost } from "../../../../shared/generated/StatsCost";
import type { StatsCostEntity } from "../../../../shared/generated/StatsCostEntity";
import type { StatsEffortCostEntity } from "../../../../shared/generated/StatsEffortCostEntity";
import type { StatsProjectCostEntity } from "../../../../shared/generated/StatsProjectCostEntity";
import { useStatsAbsorption } from "../../../store/useStatsAbsorption";
import type { CoupleRow } from "../../../model/statsAbsorption";
import type { CostAxis } from "../../../model/cost";
import { Breadcrumb } from "../common/Breadcrumb";
import type { TabFrame } from "../../StatsCharts";
import { EmptyNote, Panel } from "../common/ChartPrimitives";
import { ProvenanceName } from "../common/Provenance";
import {
  PipelineName,
  TableCombinedIcon,
} from "../common/TableCombinedIcon";
import { effortNameCell } from "./effortNameCell";
import { CostRanking } from "./CostRanking";
import { CostSummary } from "./CostSummary";
import { CostTable } from "./CostTable";
import type { CostTree } from "./costTree";
import { costSelections, type NodeLevel } from "./costSelections";
import { resolveRollUp, ROLL_UP_ROOT, type RollUpPath } from "./rollUp";
import { RollUpBadge } from "./RollUpMarks";

export function CostTab({
  cost,
  error,
  uncombined,
  onAbsorptionsChanged,
  frame,
  axis,
  onAxisChange: setAxis,
  rolledUp = false,
}: {
  cost: StatsCost | null;
  error: string | null;
  uncombined: boolean;
  onAbsorptionsChanged: () => void;
  frame: TabFrame;
  /** The grouping, held by the shell: « roll up child runs » leaves the band
   *  on « By model ». */
  axis: CostAxis;
  onAxisChange: (axis: CostAxis) => void;
  /** « Roll up child runs » is in force: a root Pipeline opens on Parent run |
   *  Child runs › Depth k › child Pipeline. */
  rolledUp?: boolean;
}) {
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
  // Inside a root Pipeline whose row carries a roll-up.
  const [rollUpPath, setRollUpPath] = useState<RollUpPath>(ROLL_UP_ROOT);

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
    setRollUpPath(ROLL_UP_ROOT);
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

  // The Pipeline row on screen (By pipeline: the selected row; By project:
  // the drilled one) and, under the roll-up, where its drill stands.
  const pipelineRow =
    axis === "pipeline" ? selected : axis === "project" ? drilledPipeline : null;
  const rollUp =
    rolledUp && pipelineRow?.roll_up
      ? resolveRollUp(pipelineRow, pipelineRow.roll_up, rollUpPath, setRollUpPath)
      : null;

  const focus =
    axis === "model"
      ? (modelPipeline ?? effort ?? model)
      : (rollUp?.focus ?? drilledPipeline ?? selected);
  const aggregate = focus ?? cost.total;
  const periods = focus?.by_period ?? cost.by_period;

  // The Nodes the table shows, on whichever axis they were reached.
  const nodePipeline =
    axis === "model" ? modelPipeline : axis === "project" ? drilledPipeline : selected;
  const nodeLevel: NodeLevel | null = rollUp
    ? rollUp.nodeLevel
    : nodePipeline && { pipeline: nodePipeline, nodes: nodePipeline.nodes };
  // The denominator the headline names (ADR-0065 §3): a model bucket counts
  // executions — the whole model axis reads "per execution" — and so does the
  // Node level of the other axes.
  const atNodeLevel = axis === "model" || nodeLevel !== null;
  const detailUnit: "Run" | "execution" = atNodeLevel ? "execution" : "Run";

  const selections = costSelections(absorption, {
    axis,
    cost,
    nodeLevel,
    model,
    effortLevel: effort === null,
  });
  const masterSelection = selections.master;
  // A Pipeline's name in a list, with the « ⑂ N » of a root under the roll-up.
  const pipelineName = (row: StatsCostEntity) => (
    <>
      <PipelineName row={row} onOpenMembers={masterSelection?.onOpenMembers} />
      {rolledUp && <RollUpBadge row={row} />}
    </>
  );

  let detailRows: StatsCostEntity[];
  let detailRenderName: ((row: StatsCostEntity) => React.ReactNode) | undefined;
  let onOpen: ((row: StatsCostEntity) => void) | undefined;
  let tree: CostTree | undefined;
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
  } else if (rollUp) {
    detailRows = rollUp.rows;
    tree = rollUp.tree;
    if (tree) {
      const level = tree;
      detailRenderName = level.renderName;
      onOpen = (row) => level.open(row, level.base)?.();
    }
  } else if (drilledPipeline) {
    detailRows = drilledPipeline.nodes;
  } else if (!selected) {
    detailRows = rows;
    if (axis === "pipeline") detailRenderName = pipelineName;
  } else if (axis === "project") {
    detailRows = (selected as StatsProjectCostEntity).pipelines;
    detailRenderName = pipelineName;
    onOpen = (pipeline) => {
      setDrilledPipelineId(pipeline.id);
      setRollUpPath(ROLL_UP_ROOT);
    };
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
        onClick: () => {
          setDrilledPipelineId(null);
          setRollUpPath(ROLL_UP_ROOT);
        },
      });
    if (drilledPipeline)
      crumbs.push({
        label: drilledPipeline.name,
        onClick: () => setRollUpPath(ROLL_UP_ROOT),
      });
    if (rollUp) crumbs.push(...rollUp.crumbs);
  }
  // Every crumb but the last pops the levels it shadows (design: clickable
  // breadcrumb for all three axes once there are four levels).
  const clickableCrumbs = crumbs.map((crumb, index) =>
    index === crumbs.length - 1 ? { label: crumb.label } : crumb,
  );

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
          <CostRanking
            axis={axis}
            onAxisChange={(next) => {
              setAxis(next);
              toTotal();
              absorption.clear();
            }}
            noCostSource={noCostSource}
            rows={rows}
            selected={axis === "model" ? selectedModelId : selectedId}
            selection={masterSelection}
            badge={
              rolledUp && axis === "pipeline"
                ? (row) => <RollUpBadge row={row} />
                : undefined
            }
            onSelect={(id) => {
              if (axis === "model") {
                setSelectedModelId(id);
                setSelectedEffortId(null);
                setSelectedPipelineId(null);
              } else {
                setSelectedId(id);
                setDrilledPipelineId(null);
                setRollUpPath(ROLL_UP_ROOT);
              }
            }}
          />

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
                key={`${axis}-${selectedId ?? ""}-${drilledPipelineId ?? ""}-${selectedModelId ?? ""}-${selectedEffortId ?? "total"}-${selectedPipelineId ?? ""}-${rollUp ? JSON.stringify(rollUpPath) : ""}`}
                rows={detailRows}
                harnesses={tableHarnesses}
                unit={detailUnit}
                onOpen={onOpen}
                renderName={detailRenderName}
                expandablePairs={axis !== "model"}
                tree={tree}
                selection={
                  detailRows === nodeLevel?.nodes
                    ? selections.node
                    : detailRows === model?.efforts
                      ? selections.effort
                      : undefined
                }
                coupleSelection={
                  detailRows === nodeLevel?.nodes ? selections.couple : undefined
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
