import { ExternalLink } from "lucide-react";
import { KeyValueRow } from "../../../shared/ui/key-value-row";
import type { InstanceSettings } from "../../model/settings";
import type { StatsOpenIntent } from "../SettingsSurface";

/**
 * Price table (#427, ADR-0034) — READ ONLY: which of the three price tiers is in force.
 * The paths are shown even when the files are absent, because nothing is ever seeded and
 * naming them is the only way a user learns where to write. The resolved read view and
 * "Sync costs" live in Stats › Cost › Pricing details, next to the numbers they change.
 */
export function PriceTableRows({
  settings,
  onOpenStats,
}: {
  settings: InstanceSettings;
  onOpenStats?: (intent: StatsOpenIntent) => void;
}) {
  const table = settings.price_table;
  // Not dead code despite the type: under `vite dev` the SPA may face an older daemon.
  if (!table) return null;
  return (
    <div className="flex flex-col gap-1.5" data-testid="setting-price-table">
      <KeyValueRow label="Manual overrides (wins)" testId="setting-price-table-manual-path">
        <span className="font-mono">{table.manual_path ?? "— (HOME unset)"}</span>
        {table.manual_keys.length > 0 && (
          <span>
            {" · "}
            {table.manual_keys.length} model{table.manual_keys.length === 1 ? "" : "s"}:{" "}
            {table.manual_keys.join(", ")}
          </span>
        )}
      </KeyValueRow>
      <KeyValueRow label="Fetched table (PDO writes this)" testId="setting-price-table-fetched-path">
        <span className="font-mono">{table.fetched_path ?? "— (HOME unset)"}</span>
        {` · ${table.fetched_rows} model${table.fetched_rows === 1 ? "" : "s"}`}
      </KeyValueRow>
      <KeyValueRow label="Last sync" testId="setting-price-table-fetched-at">
        {table.fetched_at
          ? `${table.fetched_at}${table.source ? ` from ${table.source}` : ""}`
          : "Never synced — only the built-in prices apply."}
      </KeyValueRow>
      {table.reason && (
        <KeyValueRow label="Refused" tone="failed" testId="setting-price-table-reason">
          {table.reason}
        </KeyValueRow>
      )}
      <div className="mt-1 flex flex-wrap items-center gap-3">
        <button
          type="button"
          onClick={() => onOpenStats?.({ tab: "cost", pricingOpen: true })}
          disabled={!onOpenStats}
          data-testid="settings-open-stats-pricing"
          className="flex items-center gap-1.5 rounded border border-line-strong bg-bg-3 px-2.5 py-1.5 text-fg-2 hover:border-acc disabled:opacity-40"
          style={{ fontSize: 11 }}
        >
          Open Stats › Cost › Pricing details
          <ExternalLink size={11} />
        </button>
        <span className="text-fg-3" style={{ fontSize: "10.5px" }}>
          Sync lives in Stats, next to the numbers it changes. Costs resolve{" "}
          <span className="font-mono">manual → fetched → built-in</span>.
        </span>
      </div>
    </div>
  );
}

/**
 * Harness descriptors (#553/ADR-0045) — the disk tier. Always named (nothing is seeded);
 * a broken/refused descriptor is named here, the only place, since a hand-edited file
 * passes through no validator.
 */
export function HarnessDescriptorRows({ settings }: { settings: InstanceSettings }) {
  const view = settings.harness_descriptors;
  if (!view) return null;
  return (
    <div className="flex flex-col gap-1.5" data-testid="setting-harness-descriptors">
      <KeyValueRow label="Descriptor file" testId="setting-harness-descriptors-path">
        <span className="font-mono">{view.path ?? "— (HOME unset)"}</span>
      </KeyValueRow>
      <KeyValueRow label="Harnesses" testId="setting-harness-descriptors-names">
        <span className="font-mono">{view.names.join(" · ")}</span>
      </KeyValueRow>
      {(view.rejected ?? []).map((item) => (
        <KeyValueRow
          key={item.name}
          label={<span className="font-mono">{item.name}</span>}
          tone="failed"
          testId={`setting-harness-descriptor-rejected-${item.name}`}
        >
          refused: {item.why}
        </KeyValueRow>
      ))}
      {view.reason && (
        <KeyValueRow label="Refused" tone="failed" testId="setting-harness-descriptors-reason">
          {view.reason}
        </KeyValueRow>
      )}
    </div>
  );
}
