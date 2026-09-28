import { useMemo, useState } from "react";
import type { StatsOverview } from "../../../../shared/generated/StatsOverview";
import type { StatsSessionEntity } from "../../../../shared/generated/StatsSessionEntity";
import { harnessColor } from "../../../../harness/model/color";
import type { MasterSelection } from "../../../store/absorptionSelection";
import { useStatsAbsorption } from "../../../store/useStatsAbsorption";
import { SESSIONS_COUNT } from "../../../model/rowCounts";
import { HarnessBars, HarnessLegend } from "../common/ChartPrimitives";
import { MasterList } from "../common/MasterList";
import {
  PipelineName,
  TableCombinedIcon,
} from "../common/TableCombinedIcon";
import { tableSelection } from "../common/tableSelection";

export function SessionsTab({
  overview,
  uncombined,
  onAbsorptionsChanged,
}: {
  overview: StatsOverview;
  /** « Uncombined » (#891): the raw rows are a comparison, not a place to combine. */
  uncombined: boolean;
  onAbsorptionsChanged: () => void;
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

  return (
    <div className="flex flex-col gap-4" data-testid="stats-chart-sessions">
      <HarnessLegend harnesses={overview.session_harnesses} />
      <HarnessBars
        periods={periods}
        harnesses={overview.session_harnesses}
        value="executions"
      />
      <div className="flex min-h-[220px] gap-4">
        <div className="min-w-[250px] border-r border-line pr-3">
          <MasterList
            rows={rows}
            selected={selectedId}
            valueLabel={(row) => String(row.executions)}
            onSelect={setSelectedId}
            selection={pipelines}
          />
        </div>
        <div className="min-w-0 flex-1">
          <div className="mb-3 text-fg-4" style={{ fontSize: "10.5px" }}>
            Total{selected ? ` / ${selected.name}` : ""}
          </div>
          <SessionTable
            rows={detailRows}
            harnesses={overview.session_harnesses}
            onOpenMembers={selected ? undefined : pipelines?.onOpenMembers}
            selection={nodes}
          />
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
  const order = rows.map((row) => row.id);
  return (
    <table
      className="w-full table-fixed text-left"
      style={{ fontSize: "11px" }}
    >
      <thead className="text-fg-4">
        <tr>
          <th className="pb-2 font-medium">Name</th>
          <th className="w-20 pb-2 text-right font-medium">Total</th>
          {harnesses.map((harness) => (
            <th key={harness} className="w-24 pb-2 text-right font-medium">
              <span className="inline-flex items-center gap-1">
                <span
                  className="h-2 w-2 rounded-full"
                  style={{ backgroundColor: harnessColor(harness) }}
                />
                {harness}
              </span>
            </th>
          ))}
        </tr>
      </thead>
      <tbody>
        {rows.map((row) => {
          const pick = tableSelection(selection, row, order);
          return (
            <tr
              key={row.id}
              className={`group border-t border-line text-fg-2 ${pick.className}`}
              tabIndex={0}
              data-testid="stats-session-row"
              {...pick.rowProps}
            >
              <td className="py-2 pr-2">
                <span className="inline-flex items-center gap-1.5">
                  {pick.control}
                  <PipelineName row={row} onOpenMembers={onOpenMembers} />
                  <TableCombinedIcon selection={selection} row={row} />
                </span>
              </td>
              <td className="py-2 text-right font-mono">{row.executions}</td>
              {harnesses.map((harness) => (
                <td
                  key={harness}
                  className="py-2 text-right font-mono"
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
