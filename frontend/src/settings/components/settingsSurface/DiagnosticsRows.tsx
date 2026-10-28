import { ExternalLink, RefreshCw } from "lucide-react";
import { modelCount, probedAgo } from "../../../harness/model/catalog";
import type { HarnessListItem } from "../../../harness/model/harness";
import { useRefreshModels } from "../../../harness/store/useHarnessCatalog";
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
          className="flex items-center gap-1.5 rounded border border-line-strong bg-surface-2 px-2.5 py-1.5 text-text-2 hover:border-primary disabled:opacity-40"
          style={{ fontSize: 11 }}
        >
          Open Stats › Cost › Pricing details
          <ExternalLink size={11} />
        </button>
        <span className="text-text-3" style={{ fontSize: "10.5px" }}>
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
 * passes through no validator. One row per harness: its version, how many models it
 * offers and when they were probed, and « Refresh models », which probes them again
 * even at the same version; every open browser gets the new list.
 */
export function HarnessDescriptorRows({ settings }: { settings: InstanceSettings }) {
  const { probing, failed, refresh } = useRefreshModels();
  const view = settings.harness_descriptors;
  if (!view) return null;
  // A daemon predating the per-harness entries only names them.
  const harnesses: HarnessListItem[] =
    view.harnesses ?? view.names.map((name) => ({ name, source: "builtin", installed: true }));
  return (
    <div className="flex flex-col gap-1.5" data-testid="setting-harness-descriptors">
      <KeyValueRow label="Descriptor file" testId="setting-harness-descriptors-path">
        <span className="font-mono">{view.path ?? "— (HOME unset)"}</span>
      </KeyValueRow>
      <div className="flex flex-col gap-1.5" data-testid="setting-harness-descriptors-names">
        {harnesses.map((h) => (
          <HarnessRow
            key={h.name}
            harness={h}
            probing={probing.has(h.name)}
            failure={failed[h.name] ?? null}
            onRefresh={() => void refresh(h.name)}
          />
        ))}
      </div>
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
      <p className="px-0.5 text-text-3" style={{ fontSize: "10.5px" }}>
        A refresh re-reads the models a harness offers, even at the same version; every open browser
        gets the new list.
      </p>
    </div>
  );
}

function HarnessRow({
  harness,
  probing,
  failure,
  onRefresh,
}: {
  harness: HarnessListItem;
  probing: boolean;
  failure: string | null;
  onRefresh: () => void;
}) {
  const models = harness.models ?? [];
  // Installed, yet the binary answered no version: its probe failed.
  const probeFailed = harness.installed && harness.version === null;
  const probed = probedAgo(harness.probed_at);
  const fresh = probed === "probed just now";
  return (
    <div
      className="flex items-center gap-3 rounded bg-surface-2 px-2.5 py-1.5 text-text-3"
      style={{ fontSize: "10.5px" }}
      data-testid={`setting-harness-row-${harness.name}`}
    >
      <span className="w-20 shrink-0 truncate font-mono text-text-2">{harness.name}</span>
      {!harness.installed ? (
        <span className="text-text-3">not installed</span>
      ) : (
        <>
          <span className="shrink-0 font-mono text-text-3">{harness.version ?? "—"}</span>
          <span className="shrink-0 font-mono text-text-2" data-testid={`setting-harness-models-${harness.name}`}>
            {modelCount(models.length)}
          </span>
        </>
      )}
      <span className="flex-1" />
      {failure ? (
        <span className="min-w-0 truncate text-st-failed" role="alert" data-testid={`setting-harness-failed-${harness.name}`}>
          {failure}
        </span>
      ) : probeFailed ? (
        <span className="text-st-failed" data-testid={`setting-harness-failed-${harness.name}`}>
          probe failed
        </span>
      ) : (
        harness.installed && (
          <span className={fresh ? "text-primary" : "text-text-3"} data-testid={`setting-harness-probed-${harness.name}`}>
            {probed}
          </span>
        )
      )}
      <button
        type="button"
        onClick={onRefresh}
        disabled={probing || !harness.installed}
        title={harness.installed ? "Probe the models of this harness again" : "Its binary is not installed"}
        className="flex shrink-0 items-center gap-1 rounded border border-line-strong bg-surface-1 px-2 py-0.5 text-text-2 hover:border-primary disabled:opacity-50"
        data-testid={`setting-harness-refresh-${harness.name}`}
      >
        <RefreshCw size={10} className={probing ? "animate-spin" : undefined} />
        {probing ? "Probing…" : "Refresh models"}
      </button>
    </div>
  );
}
