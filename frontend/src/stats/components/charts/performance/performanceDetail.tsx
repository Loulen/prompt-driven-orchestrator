import type { PerformanceEffortEntity } from "../../../../shared/generated/PerformanceEffortEntity";
import type { StatsModelPerformanceEntity } from "../../../../shared/generated/StatsModelPerformanceEntity";
import type { StatsPerformance } from "../../../../shared/generated/StatsPerformance";
import type { StatsPerformanceEntity } from "../../../../shared/generated/StatsPerformanceEntity";
import { Tooltip } from "../../../../shared/ui/tooltip";
import type { MasterSelection } from "../../../store/absorptionSelection";
import type { useStatsAbsorption } from "../../../store/useStatsAbsorption";
import {
  coupleRows,
  coupleScope,
  hasExplicitEffort,
} from "../../../model/statsAbsorption";
import type { PerformanceAxis } from "../../../model/performance";
import {
  NOT_SET_COPY,
  performanceProvenanceCopy,
} from "../../../model/provenance";
import { RUNS_COUNT } from "../../../model/rowCounts";
import { ProvenanceName } from "../common/Provenance";
import {
  PipelineName,
  TableCombinedIcon,
} from "../common/TableCombinedIcon";
import type { CoupleSelection } from "../common/tableSelection";

export function performanceEffortName(row: PerformanceEffortEntity): React.ReactNode {
  if (row.effort === null) {
    // The "not set" bucket: hovering the italic word explains it (ADR-0065 §1).
    return (
      <Tooltip content={NOT_SET_COPY} side="top">
        <span className="italic text-fg-4">not set</span>
      </Tooltip>
    );
  }
  return (
    <ProvenanceName
      name={row.name}
      provenance={row.provenance}
      harnesses={[]}
      target="effort"
      content={performanceProvenanceCopy(row.provenance)}
    />
  );
}

/** Where the Performance drill stands: the model axis's Model → Effort →
 *  Pipeline path, or the « By pipeline » row on screen. */
export interface PerformanceLevel {
  axis: PerformanceAxis;
  model: StatsModelPerformanceEntity | null;
  effort: PerformanceEffortEntity | null;
  modelPipeline: StatsPerformanceEntity | null;
  selected: StatsPerformanceEntity | null;
}

/** The Pipeline whose Nodes the table shows (never Infrastructure's roles). */
export function nodePipelineOf({
  axis,
  modelPipeline,
  selected,
}: PerformanceLevel): StatsPerformanceEntity | null {
  return axis === "model"
    ? modelPipeline
    : selected && selected.id !== "__infrastructure__"
      ? selected
      : null;
}

/**
 * The rows the Performance absorption may combine at this level (#890, #892,
 * #906): the « By pipeline » Pipeline rows — never the Infrastructure row
 * beside them —, the « By model » Model rows, the Nodes of the Pipeline on
 * screen, the efforts of the model on screen, and the couples of each Node.
 */
export function performanceSelections(
  absorption: Pick<ReturnType<typeof useStatsAbsorption>, "selectionFor">,
  performance: StatsPerformance,
  level: PerformanceLevel,
) {
  const { axis, model, effort } = level;
  const masterSelection =
    axis === "model"
      ? absorption.selectionFor({
          dimension: "model",
          rows: performance.by_model,
          count: RUNS_COUNT,
        })
      : absorption.selectionFor({
          dimension: "pipeline",
          rows: performance.by_pipeline,
          count: RUNS_COUNT,
        });
  const nodePipeline = nodePipelineOf(level);
  const nodeSelection = nodePipeline
    ? absorption.selectionFor({
        dimension: "node",
        scope: { key: nodePipeline.id, name: nodePipeline.name },
        rows: nodePipeline.nodes,
        count: RUNS_COUNT,
      })
    : undefined;
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
          preferred: hasExplicitEffort,
        })
    : undefined;
  return {
    masterSelection,
    nodePipeline,
    nodeSelection,
    effortSelection,
    coupleSelection,
  };
}

/** What the detail table lists at this level, and how its rows open. */
export function performanceDetail({
  level,
  performance,
  masterRows,
  masterSelection,
  visibleNodes,
  onOpenModel,
  onOpenEffort,
  onOpenPipeline,
}: {
  level: PerformanceLevel;
  performance: StatsPerformance;
  masterRows: StatsPerformanceEntity[];
  masterSelection: MasterSelection | undefined;
  visibleNodes: (rows: StatsPerformanceEntity[]) => StatsPerformanceEntity[];
  onOpenModel: (id: string) => void;
  onOpenEffort: (id: string) => void;
  onOpenPipeline: (id: string) => void;
}) {
  const { axis, model, effort, modelPipeline, selected } = level;
  let detailRows: StatsPerformanceEntity[];
  // The Node population the level would show without the kind filter — set only
  // on a Node level, so an empty table can tell « nothing here » from « the
  // filter took everything » (#810).
  let unfilteredNodes: StatsPerformanceEntity[] | null = null;
  let detailRenderName:
    ((row: StatsPerformanceEntity) => React.ReactNode) | undefined;
  let onOpen: ((row: StatsPerformanceEntity) => void) | undefined;
  if (axis === "model") {
    if (modelPipeline) {
      // Node leaves are the floor: the model × effort path is the drill.
      unfilteredNodes = modelPipeline.nodes;
      detailRows = visibleNodes(modelPipeline.nodes);
    } else if (effort) {
      detailRows = effort.pipelines;
      onOpen = (row) => onOpenPipeline(row.id);
    } else if (model) {
      detailRows = model.efforts;
      detailRenderName = (row) => {
        const match = model.efforts.find((item) => item.id === row.id);
        return match ? performanceEffortName(match) : row.name;
      };
      onOpen = (row) => onOpenEffort(row.id);
    } else {
      detailRows = performance.by_model;
      detailRenderName = (row) => {
        const provenance = performance.by_model.find(
          (m) => m.id === row.id,
        )?.provenance;
        return (
          <span className="inline-flex items-center gap-1.5">
            <ProvenanceName
              name={row.name}
              mono
              provenance={provenance}
              harnesses={[]}
              target="model"
              content={performanceProvenanceCopy(provenance)}
            />
            <TableCombinedIcon selection={masterSelection} row={row} />
          </span>
        );
      };
      onOpen = (row) => onOpenModel(row.id);
    }
  } else if (selected) {
    if (selected.id === "__infrastructure__") {
      detailRows = performance.infrastructure;
    } else {
      unfilteredNodes = selected.nodes;
      detailRows = visibleNodes(selected.nodes);
    }
  } else {
    detailRows = masterRows;
    detailRenderName = (row) => (
      <PipelineName row={row} onOpenMembers={masterSelection?.onOpenMembers} />
    );
  }
  return { detailRows, unfilteredNodes, detailRenderName, onOpen };
}
