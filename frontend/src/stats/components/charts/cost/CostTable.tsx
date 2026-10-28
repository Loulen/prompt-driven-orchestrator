import { Fragment, useState } from "react";
import { ChevronDown, ChevronRight } from "lucide-react";
import type { StatsCostEntity } from "../../../../shared/generated/StatsCostEntity";
import { harnessColor } from "../../../../harness/model/color";
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
  const sorted = [...rows].sort((a, b) => (b.usd ?? -1) - (a.usd ?? -1));
  const order = sorted.map((row) => row.id);
  // Expansion state lives HERE and dies with the table: the parent keys the
  // table on the drill path, so a stale expanded set never leaks across Nodes.
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  return (
    <>
      <table
        className="w-full table-fixed text-left"
        style={{ fontSize: "11px" }}
      >
        <thead className="text-text-3">
          <tr>
            <th className="pb-2 font-medium">Name</th>
            <th className="w-28 pb-2 text-right font-medium">Total</th>
            {harnesses.map((harness) => (
              <th key={harness} className="w-28 pb-2 text-right font-medium">
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
                  className={`group border-t border-line text-text-2 ${pick.className}`}
                  tabIndex={0}
                  data-testid="stats-detail-row"
                  {...pick.rowProps}
                >
                  <td className="py-2 pr-2">
                    <span className="inline-flex items-center gap-1">
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
                          className="shrink-0 hover:text-text"
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
                            onClick={() => onOpen(row)}
                            className="text-left hover:text-text"
                          >
                            {content}
                          </button>
                        ) : (
                          <span>{content}</span>
                        );
                      })()}
                      <TableCombinedIcon selection={selection} row={row} />
                    </span>
                  </td>
                  <td className="py-2 text-right">
                    <CostCell
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
                    <td key={harness} className="py-2 text-right">
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
                          className={`group border-t border-line bg-surface-2/40 text-text-3 ${coupled.className}`}
                          data-testid="stats-model-effort-row"
                          tabIndex={coupled.control ? 0 : undefined}
                          {...coupled.rowProps}
                        >
                          <td className="py-2 pl-7 pr-2">
                            <span className="inline-flex items-center gap-1">
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
                          <td className="py-2 text-right">
                            <CostCell unit={unit} metric={pairMetric(pair)} />
                          </td>
                          {harnesses.map((harness) => (
                            <td key={harness} className="py-2 text-right">
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
    </>
  );
}
