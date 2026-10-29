import { Fragment, useState } from "react";
import { ChevronDown, ChevronRight } from "lucide-react";
import type { StatsPerformanceEntity } from "../../../../shared/generated/StatsPerformanceEntity";
import { Tooltip } from "../../../../shared/ui/tooltip";
import type { MasterSelection } from "../../../store/absorptionSelection";
import { coupleRows } from "../../../model/statsAbsorption";
import type { DurationMode, StatsZoom } from "../../../model/statsFilters";
import {
  metricField,
  PERFORMANCE_METRICS,
  sortPerformance,
  ZOOM_HIGH,
  type PerformanceField,
  type PerformanceMetric,
} from "../../../model/performance";
import {
  NOT_SET_COPY,
  performanceProvenanceCopy,
} from "../../../model/provenance";
import { ProvenanceName } from "../common/Provenance";
import { TableCombinedIcon } from "../common/TableCombinedIcon";
import {
  coupleLine,
  tableSelection,
  type CoupleSelection,
} from "../common/tableSelection";
import { MetricCell } from "./MetricCell";
import { FilteredValue, KindBadges } from "./NodeKindMarks";
import { PerformanceTableHead } from "./PerformanceTableHead";

export function PerformanceTable({
  rows,
  harnesses,
  sort,
  onSortChange,
  mode,
  zoom,
  independentAxis,
  renderName,
  onOpen,
  expandablePairs = false,
  filteredRowIds,
  selection,
  coupleSelection,
}: {
  rows: StatsPerformanceEntity[];
  harnesses: string[];
  sort: PerformanceMetric;
  /** A click on a metric's header sorts by it, like the ranking's select. */
  onSortChange?: (sort: PerformanceMetric) => void;
  /** Which of the three readings the Duration column shows (#819) — header,
   *  plots, tooltips and the row's « −…s wait » alike. */
  mode: DurationMode;
  /** Section-level, from `PerformanceTab` — the whole table draws one level. */
  zoom: StatsZoom;
  /** Each row on its own axis instead of one axis per metric (#811). */
  independentAxis: boolean;
  /** Replaces the default (button-or-plain) name cell — the model axis marks
   *  provenance and renders "not set" in its own voice (#737). */
  renderName?: (row: StatsPerformanceEntity) => React.ReactNode;
  onOpen?: (row: StatsPerformanceEntity) => void;
  /** Node rows gain a chevron unfolding their model × effort couples (ADR-0065).
   *  Off on the model axis, where the model × effort path is already the drill. */
  expandablePairs?: boolean;
  /** Rows whose aggregate pooled executions the kind filter now hides (#810):
   *  their cells read « filtered » rather than a total that no longer describes
   *  what is on screen. */
  filteredRowIds?: Set<string>;
  /** Node rows under one Pipeline (#892), or the efforts of one model (#906):
   *  select and combine them. */
  selection?: MasterSelection;
  /** The couples of each Node row (#906): select and combine them. */
  coupleSelection?: CoupleSelection;
}) {
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  // The couples' own expansion, dying with the table like `expanded` — the
  // parent keys the table on the drill path, so a stale set never leaks.
  const [couplesExpanded, setCouplesExpanded] = useState<Set<string>>(
    new Set(),
  );
  const ordered = sortPerformance(rows, sort, mode);
  // A metric's axis tops out at the chosen level's high statistic over the
  // entities it covers — `max`, `fence_high` or `q3` (#811). `1` as a floor so
  // an all-zero metric never divides by zero.
  const axisMax = (
    entities: StatsPerformanceEntity[],
    field: PerformanceField,
  ) =>
    Math.max(
      1,
      ...entities.flatMap((row) =>
        row.harnesses.map((item) => item[field].stats?.[ZOOM_HIGH[zoom]] ?? 0),
      ),
    );
  const fieldByMetric = (metric: PerformanceMetric) =>
    metricField(metric, mode);
  const scalesOf = (
    entities: StatsPerformanceEntity[],
  ): Record<PerformanceMetric, number> => ({
    context: axisMax(entities, "context"),
    // #810: each reading has its own max — zooming out to the wall-clock scale
    // would leave every active (or waiting) box crushed against the left edge.
    duration: axisMax(entities, fieldByMetric("duration")),
    steering: axisMax(entities, "steering"),
  });
  // The shared scale is drawn from the rows and their subagents — a couple's
  // peaks/durations come from those same session files, so they fit it. The
  // independent one applies the very same rule one row at a time, subagents
  // folded into their Node's row (issue AC).
  const sharedScales = scalesOf(rows.flatMap((row) => [row, ...row.subagents]));
  const scalesByRow = new Map(
    ordered.map((row) => [row.id, scalesOf([row, ...row.subagents])] as const),
  );
  const scalesFor = (rowId: string) =>
    independentAxis ? (scalesByRow.get(rowId) ?? sharedScales) : sharedScales;

  const visible = ordered.flatMap((row) => [
    { row, child: false, scales: scalesFor(row.id) },
    ...(expanded.has(row.id)
      ? sortPerformance(row.subagents, sort, mode).map((child) => ({
          row: child,
          child: true,
          // A subagent reads its parent's axis: it belongs to that Node's row.
          scales: scalesFor(row.id),
        }))
      : []),
  ]);

  const cellProps = { harnesses, mode, zoom, independentAxis };

  return (
    <div className="sx-hscroll">
      <table className="sx-table sx-table--perf">
        <PerformanceTableHead mode={mode} sort={sort} onSortChange={onSortChange} />
        <tbody>
          {visible.map(({ row, child, scales }) => {
            const couples = !child && expandablePairs ? (row.models ?? []) : [];
            // A subagent group is no Node: it never combines.
            const rowSelection = child ? undefined : selection;
            const pick = tableSelection(
              rowSelection,
              row,
              ordered.map((item) => item.id),
            );
            const coupleLines = coupleLine(
              child ? undefined : coupleSelection?.(row),
              coupleRows(couples),
            );
            return (
              <Fragment key={`${child ? "subagent" : "entity"}-${row.id}`}>
                <tr
                  className={`group ${child ? "sx-sub" : ""}`}
                  data-testid="stats-detail-row"
                  tabIndex={rowSelection ? 0 : undefined}
                  {...pick.rowProps}
                >
                  <td>
                    <span className="sx-name">
                      {pick.control}
                      {couples.length > 0 ? (
                        <button
                          type="button"
                          aria-label={`${couplesExpanded.has(row.id) ? "Collapse" : "Expand"} ${row.name} models`}
                          data-testid="stats-node-toggle"
                          onClick={() =>
                            setCouplesExpanded((current) => {
                              const next = new Set(current);
                              if (next.has(row.id)) next.delete(row.id);
                              else next.add(row.id);
                              return next;
                            })
                          }
                          className="sx-chev"
                        >
                          {couplesExpanded.has(row.id) ? (
                            <ChevronDown size={12} />
                          ) : (
                            <ChevronRight size={12} />
                          )}
                        </button>
                      ) : null}
                      {!child && row.subagents.length > 0 ? (
                        <button
                          type="button"
                          aria-label={`${expanded.has(row.id) ? "Collapse" : "Expand"} ${row.name} subagents`}
                          onClick={() =>
                            setExpanded((current) => {
                              const next = new Set(current);
                              if (next.has(row.id)) next.delete(row.id);
                              else next.add(row.id);
                              return next;
                            })
                          }
                          className="sx-chev"
                        >
                          {expanded.has(row.id) ? (
                            <ChevronDown size={12} />
                          ) : (
                            <ChevronRight size={12} />
                          )}
                        </button>
                      ) : null}
                      {(() => {
                        const content = renderName ? renderName(row) : row.name;
                        return onOpen ? (
                          <button
                            type="button"
                            aria-label={`Open ${row.name}`}
                            title={row.name}
                            onClick={() => onOpen(row)}
                            className="sx-open"
                          >
                            {content}
                          </button>
                        ) : renderName ? (
                          content
                        ) : (
                          <span className="sx-ellipsis" title={row.name}>
                            {content}
                          </span>
                        );
                      })()}
                      <TableCombinedIcon selection={rowSelection} row={row} />
                      {!child && <KindBadges row={row} />}
                    </span>
                  </td>
                  {filteredRowIds?.has(row.id)
                    ? PERFORMANCE_METRICS.map((metric) => (
                        <td key={metric}>
                          <FilteredValue testid="stats-performance-filtered-cell" />
                        </td>
                      ))
                    : PERFORMANCE_METRICS.map((metric) => (
                        <MetricCell
                          key={metric}
                          {...cellProps}
                          name={row.name}
                          rowHarnesses={row.harnesses}
                          metric={metric}
                          scales={scales}
                        />
                      ))}
                </tr>
                {couplesExpanded.has(row.id)
                  ? couples.map((pair, index) => {
                      const couple = coupleLines.rows[index];
                      const coupled = coupleLines.pick(couple);
                      return (
                        <tr
                          key={`${row.id}-${pair.key}`}
                          className="group sx-sub"
                          data-testid="stats-performance-model-effort-row"
                          tabIndex={coupled.control ? 0 : undefined}
                          {...coupled.rowProps}
                        >
                          <td>
                            <span className="sx-name">
                              {coupled.control}
                              <span className="inline-flex items-baseline gap-1">
                                <ProvenanceName
                                  name={pair.model}
                                  mono
                                  provenance={pair.model_provenance}
                                  harnesses={[]}
                                  target="model"
                                  content={performanceProvenanceCopy(
                                    pair.model_provenance,
                                  )}
                                />
                                <span className="text-text-3">·</span>
                                {pair.effort === null ? (
                                  <Tooltip content={NOT_SET_COPY} side="top">
                                    <span className="italic text-text-3">
                                      not set
                                    </span>
                                  </Tooltip>
                                ) : (
                                  <ProvenanceName
                                    name={pair.effort}
                                    provenance={pair.effort_provenance}
                                    harnesses={[]}
                                    target="effort"
                                    content={performanceProvenanceCopy(
                                      pair.effort_provenance,
                                    )}
                                  />
                                )}
                              </span>
                              {coupleLines.marks(couple)}
                            </span>
                          </td>
                          {PERFORMANCE_METRICS.map((metric) => (
                            <MetricCell
                              key={metric}
                              {...cellProps}
                              name={`${pair.model} · ${pair.effort ?? "not set"}`}
                              rowHarnesses={pair.harnesses}
                              metric={metric}
                              scales={scales}
                            />
                          ))}
                        </tr>
                      );
                    })
                  : null}
              </Fragment>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
