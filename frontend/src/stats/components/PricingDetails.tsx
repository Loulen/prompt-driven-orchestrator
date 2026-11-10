import type { PriceRow } from "../../shared/generated/PriceRow";
import type { StatsCost } from "../../shared/generated/StatsCost";
import type { SyncCostPricesReport } from "../model/prices";

function PriceRows({ rows }: { rows: PriceRow[] }) {
  return (
    <div className="flex flex-col">
      {rows.map((row) => (
        <div
          key={row.key}
          className="flex items-center justify-between gap-3 border-b border-line-soft px-1 py-1 font-mono text-xs text-text-3"
        >
          <span>{row.key}</span>
          <span>
            <span className="mr-2 text-text-3">{row.tier}</span>
            <span className="text-text-2">${row.input}/${row.output} /MTok</span>
          </span>
        </div>
      ))}
    </div>
  );
}

function SyncResult({ report }: { report: SyncCostPricesReport | null }) {
  if (!report) return null;
  if (report.noop) {
    return (
      <div className="text-text-3" data-testid="stats-sync-noop">
        {report.reason ?? "Price table already up to date."}
      </div>
    );
  }
  return (
    <div
      className="border border-st-await/40 px-3 py-2 text-text-2"
      data-testid="stats-sync-report"
    >
      <div>
        {report.rows} price row(s) from <span className="font-mono">{report.source}</span>
        {report.fetched_at ? ` at ${report.fetched_at}` : ""}.
      </div>
      <ul className="list-disc pl-4">
        {report.added.length > 0 && <li>Newly priced: {report.added.join(", ")}</li>}
        {report.updated.length > 0 && <li>Price changed: {report.updated.join(", ")}</li>}
        {report.shadowed_by_manual.length > 0 && (
          <li>
            Kept from your <span className="font-mono">models.yaml</span> (overrides the fetched
            price): {report.shadowed_by_manual.join(", ")}
          </li>
        )}
        {report.rejected.length > 0 && <li>Refused by the source: {report.rejected.join("; ")}</li>}
      </ul>
    </div>
  );
}

/** The body of Stats' secondary panel: « Sync costs » lives in the panel header (#944). */
export function PricingDetailsContent({
  cost,
  syncError,
  syncReport,
}: {
  cost: StatsCost | null;
  syncError: string | null;
  syncReport: SyncCostPricesReport | null;
}) {
  const warnings = cost
    ? [...cost.total.unpriced_models, ...cost.total.missing_reasons]
    : [];
  return (
    <div className="flex flex-col gap-3 text-xs">
      {syncError && (
        <div
          className="sx-err"
          data-testid="stats-sync-error"
        >
          {syncError}
        </div>
      )}
      <SyncResult report={syncReport} />
      {warnings.map((warning) => (
        <div key={warning} className="text-st-await">
          {warning}
        </div>
      ))}
      {cost?.resolved.length ? (
        <PriceRows rows={cost.resolved} />
      ) : (
        <div className="text-text-3">No resolved prices.</div>
      )}
    </div>
  );
}
