import { Fragment, useState, type CSSProperties } from "react";
import { ChevronDown, ChevronRight } from "lucide-react";
import type { StatsCostEntity } from "../../../../shared/generated/StatsCostEntity";
import { harnessColor } from "../../../../harness/model/color";
import { InBar, SortTh } from "../common/SortHeader";
import { sortRows, useTableSort } from "../common/tableSort";
import { Tooltip } from "../../../../shared/ui/tooltip";
import type { MasterSelection } from "../../../store/absorptionSelection";
import { coupleRows } from "../../../model/statsAbsorption";
import { pairMetric } from "../../../model/cost";
import { NOT_SET_COPY } from "../../../model/provenance";
import { ProvenanceName } from "../common/Provenance";
import { TableCombinedIcon } from "../common/TableCombinedIcon";
import {
  coupleLine,
  tableSelection,
  type CoupleSelection,
} from "../common/tableSelection";
import { CostCell } from "./CostCells";

export function CostTable({
  rows,
  harnesses,
  unit,
  onOpen,
  renderName,
  expandablePairs = false,
  selection,
  coupleSelection,
}: {
  rows: StatsCostEntity[];
  harnesses: string[];
  unit: "Run" | "execution";
  /** Node rows under one Pipeline (#892), or the efforts of one model (#906):
   *  select and combine them. */
  selection?: MasterSelection;
  /** The couples of each Node row (#906): select and combine them. */
  coupleSelection?: CoupleSelection;
  onOpen?: (row: StatsCostEntity) => void;
  /** Replaces the default (button-or-plain) name cell — the model axis marks
   *  provenance and renders "not set" in its own voice. */
  renderName?: (row: StatsCostEntity) => React.ReactNode;
  /** Node rows gain a chevron unfolding their model × effort pairs (ADR-0065).
   *  Off on the model axis, where the model × effort path is already the drill. */
  expandablePairs?: boolean;
}) {
  // Sorted by cost at first (`overlays.statsBreakdown`); a header sorts by its column.
  const [sort, onSort] = useTableSort({ key: "total", dir: "desc" });
  const sorted = sortRows(rows, sort, (row, key) =>
    key === "name"
      ? row.name
      : key === "total"
        ? row.usd
        : (row.harnesses.find((item) => item.harness === key)?.usd ?? null),
  );
  const order = sorted.map((row) => row.id);
  const max = Math.max(0, ...rows.map((row) => row.usd ?? 0));
  const topId = rows.find((row) => (row.usd ?? 0) === max && max > 0)?.id;
  // Expansion state lives HERE and dies with the table: the parent keys the
  // table on the drill path, so a stale expanded set never leaks across Nodes.
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  return (
    <div className="sx-hscroll">
      <table className="sx-table sx-table--cost" style={{ "--sx-cols": harnesses.length } as CSSProperties}>
        <thead>
          <tr>
            <SortTh sort={sort} onSort={onSort} sortKey="name">
              Name
            </SortTh>
            <SortTh sort={sort} onSort={onSort} sortKey="total" className="sx-r w-32">
              Total
            </SortTh>
            {harnesses.map((harness) => (
              <SortTh key={harness} sort={sort} onSort={onSort} sortKey={harness} className="sx-r w-28">
                <span className="sx-swatch" style={{ backgroundColor: harnessColor(harness) }} aria-hidden />
                {harness}
              </SortTh>
            ))}
          </tr>
        </thead>
        <tbody>
          {sorted.map((row) => {
            const pairs = expandablePairs ? (row.models ?? []) : [];
            const pick = tableSelection(selection, row, order);
            const couples = coupleLine(
              coupleSelection?.(row),
              coupleRows(pairs),
            );
            return (
              <Fragment key={row.id}>
                <tr
                  className="group"
                  tabIndex={0}
                  data-testid="stats-detail-row"
                  {...pick.rowProps}
                >
                  <td>
                    <span className="sx-name">
                      {pick.control}
                      {pairs.length > 0 ? (
                        <button
                          type="button"
                          aria-label={`${expanded.has(row.id) ? "Collapse" : "Expand"} ${row.name} models`}
                          data-testid="stats-node-toggle"
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
                      <TableCombinedIcon selection={selection} row={row} />
                    </span>
                  </td>
                  <td className="sx-r">
                    <CostCell
                      bar={<InBar value={row.usd} max={max} top={row.id === topId} />}
                      unit={unit}
                      metric={{
                        harness: "total",
                        usd: row.usd,
                        estimated: row.estimated,
                        partial: row.partial,
                        executions: row.executions,
                        readable: row.readable,
                        unknown: row.unknown,
                        average_usd: row.average_usd,
                        median_usd: row.median_usd,
                        unpriced_models: row.unpriced_models,
                        missing_reasons: row.missing_reasons,
                      }}
                    />
                  </td>
                  {harnesses.map((harness) => (
                    <td key={harness} className="sx-r">
                      <CostCell
                        unit={unit}
                        metric={row.harnesses.find(
                          (item) => item.harness === harness,
                        )}
                      />
                    </td>
                  ))}
                </tr>
                {expanded.has(row.id)
                  ? pairs.map((pair, index) => {
                      const couple = couples.rows[index];
                      const coupled = couples.pick(couple);
                      return (
                        <tr
                          key={`${row.id}-${pair.key}`}
                          className="group sx-sub"
                          data-testid="stats-model-effort-row"
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
                                  harnesses={pair.harnesses}
                                  target="model"
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
                                    harnesses={pair.harnesses}
                                    target="effort"
                                  />
                                )}
                              </span>
                              {couples.marks(couple)}
                            </span>
                          </td>
                          <td className="sx-r">
                            <CostCell unit={unit} metric={pairMetric(pair)} />
                          </td>
                          {harnesses.map((harness) => (
                            <td key={harness} className="sx-r">
                              <CostCell
                                unit={unit}
                                metric={pair.harnesses.find(
                                  (item) => item.harness === harness,
                                )}
                              />
                            </td>
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
