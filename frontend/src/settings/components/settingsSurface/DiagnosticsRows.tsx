import { Check, CircleX, ExternalLink, RefreshCw } from "lucide-react";
import type { ReactNode } from "react";
import { modelCount, probedAgo } from "../../../harness/model/catalog";
import type { HarnessListItem } from "../../../harness/model/harness";
import { useRefreshModels } from "../../../harness/store/useHarnessCatalog";
import { IconButton } from "../../../shared/ui/kit";
import { KeyValueRow } from "../../../shared/ui/key-value-row";
import type { InstanceSettings } from "../../model/settings";
import type { SettingsSection } from "../../model/settingsCategories";
import type { StatsOpenIntent } from "../SettingsSurface";
import Section, { SectionExtra } from "./Section";

/**
 * Price table (#427, ADR-0034) — READ ONLY: which of the three price tiers is in force
 * (`manual → fetched → built-in`). The paths are shown even when the files are absent,
 * because nothing is ever seeded and naming them is the only way a user learns where to
 * write. The resolved read view and « Sync costs » live in Stats › Cost › Pricing details,
 * next to the numbers they change.
 */
export function PriceTableRows({
  section,
  settings,
  onOpenStats,
}: {
  section: SettingsSection;
  settings: InstanceSettings;
  onOpenStats?: (intent: StatsOpenIntent) => void;
}) {
  const table = settings.price_table;
  return (
    <Section
      section={section}
      tools={
        <IconButton
          icon={ExternalLink}
          label="Stats › Cost › Pricing details"
          size="sm"
          onClick={() => onOpenStats?.({ tab: "cost", pricingOpen: true })}
          disabled={!onOpenStats}
          data-testid="settings-open-stats-pricing"
        />
      }
    >
      {/* Not dead code despite the type: under `vite dev` the SPA may face an older daemon. */}
      {table && (
        <SectionExtra testId="setting-price-table">
          <KeyValueRow
            label="Manual overrides"
            title={table.manual_keys.length > 0 ? table.manual_keys.join(", ") : "Wins over the other tiers"}
            testId="setting-price-table-manual-path"
          >
            <span className="set-mono set-ellipsis">{table.manual_path ?? "— (HOME unset)"}</span>
            {table.manual_keys.length > 0 && (
              <span className="set-faint">
                {table.manual_keys.length} model{table.manual_keys.length === 1 ? "" : "s"}
              </span>
            )}
          </KeyValueRow>
          <KeyValueRow label="Fetched table" testId="setting-price-table-fetched-path">
            <span className="set-mono set-ellipsis">{table.fetched_path ?? "— (HOME unset)"}</span>
            <span className="set-faint">
              {table.fetched_rows} model{table.fetched_rows === 1 ? "" : "s"}
            </span>
          </KeyValueRow>
          <KeyValueRow label="Last sync" testId="setting-price-table-fetched-at">
            {table.fetched_at ? (
              <span className="set-ellipsis">
                {table.fetched_at}
                {table.source && <span className="set-faint"> · {table.source}</span>}
              </span>
            ) : (
              <span className="set-faint" title="Only the built-in prices apply">
                Never
              </span>
            )}
          </KeyValueRow>
          {table.reason && (
            <KeyValueRow label="Refused" tone="failed" title={table.reason} testId="setting-price-table-reason">
              <span className="set-ellipsis">{table.reason}</span>
            </KeyValueRow>
          )}
        </SectionExtra>
      )}
    </Section>
  );
}

/**
 * Harness descriptors (#553/ADR-0045) — the disk tier. Always named (nothing is seeded);
 * a broken/refused descriptor is named here, the only place, since a hand-edited file
 * passes through no validator. One row per harness: its version, how many models it
 * offers and when they were probed, and « Refresh models », which probes them again
 * even at the same version; every open browser gets the new list.
 */
export function HarnessDescriptorRows({ section, settings }: { section: SettingsSection; settings: InstanceSettings }) {
  const { probing, failed, refresh } = useRefreshModels();
  const view = settings.harness_descriptors;
  // A daemon predating the per-harness entries only names them.
  const harnesses: HarnessListItem[] = view
    ? (view.harnesses ?? view.names.map((name) => ({ name, source: "builtin", installed: true })))
    : [];
  return (
    <Section section={section}>
      {view && (
        <SectionExtra testId="setting-harness-descriptors">
          <KeyValueRow label="Descriptor file" testId="setting-harness-descriptors-path">
            <span className="set-mono set-ellipsis">{view.path ?? "— (HOME unset)"}</span>
          </KeyValueRow>
          <table className="set-table" data-testid="setting-harness-descriptors-names">
            <thead>
              <tr>
                <th aria-label="State" />
                <th>Harness</th>
                <th>Version</th>
                <th>Models</th>
                <th>Probed</th>
                <th aria-label="Refresh" />
              </tr>
            </thead>
            <tbody>
              {harnesses.map((h) => (
                <HarnessRow
                  key={h.name}
                  harness={h}
                  probing={probing.has(h.name)}
                  failure={failed[h.name] ?? null}
                  onRefresh={() => void refresh(h.name)}
                />
              ))}
              {(view.rejected ?? []).map((item) => (
                <tr key={item.name} data-testid={`setting-harness-descriptor-rejected-${item.name}`}>
                  <td>
                    <CircleX size={13} strokeWidth={1.5} className="set-bad" aria-label="refused" />
                  </td>
                  <td className="set-mono">{item.name}</td>
                  <td colSpan={4} className="set-bad" title={item.why}>
                    refused: {item.why}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          {view.reason && (
            <KeyValueRow label="Refused" tone="failed" title={view.reason} testId="setting-harness-descriptors-reason">
              <span className="set-ellipsis">{view.reason}</span>
            </KeyValueRow>
          )}
        </SectionExtra>
      )}
    </Section>
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
  const bad = !!failure || probeFailed;
  return (
    <tr data-testid={`setting-harness-row-${harness.name}`}>
      <td>
        {bad ? (
          <CircleX size={13} strokeWidth={1.5} className="set-bad" aria-label="failed" />
        ) : harness.installed ? (
          <Check size={13} strokeWidth={1.5} className="set-ok" aria-label="loaded" />
        ) : null}
      </td>
      <td className="set-mono">{harness.name}</td>
      {!harness.installed ? (
        <td colSpan={2} className="set-faint">
          not installed
        </td>
      ) : (
        <>
          <td className="set-mono set-faint">{harness.version ?? "—"}</td>
          <td className="set-mono" data-testid={`setting-harness-models-${harness.name}`}>
            {modelCount(models.length)}
          </td>
        </>
      )}
      <td>
        {failure ? (
          <span className="set-bad" role="alert" title={failure} data-testid={`setting-harness-failed-${harness.name}`}>
            {failure}
          </span>
        ) : probeFailed ? (
          <span className="set-bad" data-testid={`setting-harness-failed-${harness.name}`}>
            probe failed
          </span>
        ) : (
          harness.installed && (
            <span className={fresh ? "set-ok" : "set-faint"} data-testid={`setting-harness-probed-${harness.name}`}>
              {probed}
            </span>
          )
        )}
      </td>
      <td className="set-table__act">
        <IconButton
          icon={RefreshCw}
          label={probing ? "Probing…" : "Refresh models"}
          size="sm"
          className={probing ? "[&>svg]:animate-spin" : undefined}
          onClick={onRefresh}
          disabled={probing || !harness.installed}
          description={harness.installed ? undefined : "Its binary is not installed"}
          data-testid={`setting-harness-refresh-${harness.name}`}
        />
      </td>
    </tr>
  );
}

/** Settings › Diagnostics: read-only, once `GET /settings` answered. */
export function DiagnosticsCategory({
  sections,
  settings,
  onOpenStats,
  loading,
}: {
  sections: SettingsSection[];
  settings: InstanceSettings | null;
  onOpenStats?: (intent: StatsOpenIntent) => void;
  loading: ReactNode;
}) {
  if (!settings) return <>{loading}</>;
  return (
    <>
      <PriceTableRows section={sections[0]} settings={settings} onOpenStats={onOpenStats} />
      <HarnessDescriptorRows section={sections[1]} settings={settings} />
    </>
  );
}
