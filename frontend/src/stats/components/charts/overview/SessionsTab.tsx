import { useMemo, useState } from "react";
import type { StatsOverview } from "../../../../shared/generated/StatsOverview";
import type { StatsSessionEntity } from "../../../../shared/generated/StatsSessionEntity";
import { harnessColor } from "../../../../harness/model/color";
import type { MasterSelection } from "../../../store/absorptionSelection";
import { useStatsAbsorption } from "../../../store/useStatsAbsorption";
import { SESSIONS_COUNT } from "../../../model/rowCounts";
import type { TabFrame } from "../../StatsCharts";
import { Breadcrumb } from "../common/Breadcrumb";
import { HarnessBars, Kpis, Panel } from "../common/ChartPrimitives";
import { MasterList } from "../common/MasterList";
import { InBar, SortTh } from "../common/SortHeader";
import { sortRows, useTableSort } from "../common/tableSort";
import {
  PipelineName,
  TableCombinedIcon,
} from "../common/TableCombinedIcon";
import { tableSelection } from "../common/tableSelection";

export function SessionsTab({
  overview,
  uncombined,
  onAbsorptionsChanged,
  frame,
}: {
  overview: StatsOverview;
  /** « Uncombined » (#891): the raw rows are a comparison, not a place to combine. */
  uncombined: boolean;
  onAbsorptionsChanged: () => void;
  frame: TabFrame;
}) {
  const rows = useMemo(
    () =>
      [...overview.sessions_by_pipeline].sort(
        (a, b) => b.executions - a.executions,
      ),
    [overview.sessions_by_pipeline],
  );
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const selected = rows.find((row) => row.id === selectedId) ?? null;
  const periods = selected ? selected.by_period : overview.sessions_by_period;
  const detailRows = selected?.nodes ?? rows;
  const absorption = useStatsAbsorption({
    enabled: !uncombined,
    onChanged: onAbsorptionsChanged,
  });
  const pipelines = absorption.selectionFor({
    dimension: "pipeline",
    rows,
    count: SESSIONS_COUNT,
  });
  // #892: the Nodes of the Pipeline on screen, scoped to its row.
  const nodes = selected
    ? absorption.selectionFor({
        dimension: "node",
        scope: { key: selected.id, name: selected.name },
        rows: selected.nodes,
        count: SESSIONS_COUNT,
      })
    : undefined;
  const scope = selected ?? null;
  const total = scope
    ? scope.executions
    : rows.reduce((sum, row) => sum + row.executions, 0);
  const byHarness = overview.session_harnesses.map((harness) => ({
    harness,
    executions: (scope?.harnesses ?? rows.flatMap((row) => row.harnesses))
      .filter((item) => item.harness === harness)
      .reduce((sum, item) => sum + item.executions, 0),
  }));

  return (
    <div className="sx-tab" data-testid="stats-chart-sessions">
      {frame.band(
        <Breadcrumb
          testid="stats-sessions-breadcrumb"
          crumbs={
            selected
              ? [{ label: "Total", onClick: () => setSelectedId(null) }, { label: selected.name }]
              : [{ label: "Total" }]
          }
        />,
      )}
      <div className="sx-page">
        {frame.cohort}
        <Kpis
          items={[
            { key: "sessions", label: "Sessions", value: total },
            ...byHarness.map(({ harness, executions }) => ({
              key: harness,
              label: harness,
              value: executions,
              swatch: harnessColor(harness),
            })),
          ]}
        />
        <HarnessBars
          title="Sessions by harness"
          periods={periods}
          harnesses={overview.session_harnesses}
          value="executions"
        />
        <div className="sx-split">
          <Panel title="By pipeline" count={rows.length} flush className="sx-aside">
            <MasterList
              rows={rows}
              selected={selectedId}
              valueLabel={(row) => String(row.executions)}
              onSelect={setSelectedId}
              selection={pipelines}
            />
          </Panel>
          <div className="sx-detail">
            <Panel
              title={selected ? `${selected.name} · by node` : "By pipeline"}
              count={detailRows.length}
              flush
            >
              <SessionTable
                rows={detailRows}
                harnesses={overview.session_harnesses}
                onOpenMembers={selected ? undefined : pipelines?.onOpenMembers}
                selection={nodes}
              />
            </Panel>
          </div>
        </div>
      </div>
      {absorption.overlay}
    </div>
  );
}

export function SessionTable({
  rows,
  harnesses,
  onOpenMembers,
  selection,
}: {
  rows: StatsSessionEntity[];
  harnesses: string[];
  /** Pipeline rows at Total level: the combined icon opens the members. */
  onOpenMembers?: (id: string) => void;
  /** Node rows under one Pipeline (#892): select and combine them. */
  selection?: MasterSelection;
}) {
  const [sort, onSort] = useTableSort({ key: "total", dir: "desc" });
  const sorted = sortRows(rows, sort, (row, key) =>
    key === "name"
      ? row.name
      : key === "total"
        ? row.executions
        : (row.harnesses.find((item) => item.harness === key)?.executions ?? null),
  );
  const order = sorted.map((row) => row.id);
  const max = Math.max(0, ...rows.map((row) => row.executions));
  // The largest share is marked whatever the sort, as on Cost.
  const top = rows.find((row) => row.executions === max && max > 0)?.id;
  return (
    <table className="sx-table">
      <thead>
        <tr>
          <SortTh sort={sort} onSort={onSort} sortKey="name">
            Name
          </SortTh>
          <SortTh sort={sort} onSort={onSort} sortKey="total" className="sx-r w-28">
            Total
          </SortTh>
          {harnesses.map((harness) => (
            <SortTh key={harness} sort={sort} onSort={onSort} sortKey={harness} className="sx-r w-24">
              <span className="sx-swatch" style={{ backgroundColor: harnessColor(harness) }} aria-hidden />
              {harness}
            </SortTh>
          ))}
        </tr>
      </thead>
      <tbody>
        {sorted.map((row) => {
          const pick = tableSelection(selection, row, order);
          return (
            <tr
              key={row.id}
              className="group"
              tabIndex={0}
              data-testid="stats-session-row"
              {...pick.rowProps}
            >
              <td>
                <span className="sx-name">
                  {pick.control}
                  <PipelineName row={row} onOpenMembers={onOpenMembers} />
                  <TableCombinedIcon selection={selection} row={row} />
                </span>
              </td>
              <td className="sx-r sx-barcell">
                <InBar value={row.executions} max={max} top={row.id === top} />
                <span>{row.executions}</span>
              </td>
              {harnesses.map((harness) => (
                <td
                  key={harness}
                  className="sx-r"
                  data-harness={harness}
                >
                  {row.harnesses.find((item) => item.harness === harness)
                    ?.executions ?? "—"}
                </td>
              ))}
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}
