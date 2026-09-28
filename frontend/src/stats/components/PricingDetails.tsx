import type { PriceRow } from "../../shared/generated/PriceRow";
import type { StatsCost } from "../../shared/generated/StatsCost";
import type { SyncCostPricesReport } from "../model/prices";

function PriceRows({ rows }: { rows: PriceRow[] }) {
  return (
    <div className="flex flex-col gap-1.5">
      {rows.map((row) => (
        <div
          key={row.key}
          className="flex items-center justify-between rounded bg-bg-3 px-2 py-1 font-mono text-fg-3"
          style={{ fontSize: "10.5px" }}
        >
          <span>{row.key}</span>
          <span>
            <span className="mr-2 text-fg-4">{row.tier}</span>
            <span className="text-fg-2">${row.input}/${row.output} /MTok</span>
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
      <div className="text-fg-4" data-testid="stats-sync-noop">
        {report.reason ?? "Price table already up to date."}
      </div>
    );
  }
  return (
    <div
      className="rounded-md border border-st-await/40 bg-st-await/10 px-3 py-2 text-fg-2"
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

export function PricingDetails({
  cost,
  syncing,
  syncError,
  syncReport,
  onSync,
}: {
  cost: StatsCost | null;
  syncing: boolean;
  syncError: string | null;
  syncReport: SyncCostPricesReport | null;
  onSync: () => void;
}) {
  const warnings = cost
    ? [...cost.total.unpriced_models, ...cost.total.missing_reasons]
    : [];
  return (
    <aside
      className="absolute inset-y-0 right-0 z-20 w-[min(420px,90vw)] overflow-y-auto border-l border-line bg-bg-4 p-4 shadow-2xl"
      data-testid="stats-pricing-details"
    >
      <div className="mb-4 flex items-center justify-between">
        <h3 className="font-semibold text-fg">Pricing details</h3>
        <button
          type="button"
          onClick={onSync}
          disabled={syncing}
          data-testid="stats-sync-prices"
          className="rounded-md border border-line-strong bg-bg-3 px-2 py-1 text-fg-2 disabled:opacity-40"
        >
          {syncing ? "Syncing…" : "Sync costs"}
        </button>
      </div>
      <div className="flex flex-col gap-3" style={{ fontSize: "10.5px" }}>
        {syncError && (
          <div
            className="rounded-md border border-st-failed/30 bg-st-failed-bg px-3 py-2 text-st-failed"
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
          <div className="text-fg-4">No resolved prices.</div>
        )}
      </div>
    </aside>
  );
}
