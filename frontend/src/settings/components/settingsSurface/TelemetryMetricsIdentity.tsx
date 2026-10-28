/**
 * Settings › Telemetry (ADR-0090), the rows beside the OTLP export: `/metrics`, read by
 * whoever reaches the instance, by its own switch; « include the Actor », with what it
 * obliges an employer to.
 */

import { ShieldAlert } from "lucide-react";
import { Switch } from "../../../shared/ui/kit";
import type { InstanceSettings } from "../../model/settings";
import { telemetryFlagSourceNote } from "../../model/sourceNotes";
import { SettingRow, SwitchRow } from "./fields";
import type { FormSectionProps } from "./GeneralSections";

/** ADR-0090 §4: what measuring each person's activity obliges an employer to. */
const LEGAL =
  "In France, before you switch it on: the people concerned must be informed beforehand (Code du travail, art. L1222-4), the CSE consulted (art. L2312-38), and the processing must have a GDPR basis and be in your record of processing. This instance cannot meet these obligations for you.";

/**
 * The address people open the instance at: the Public URL when the daemon names one, else
 * the address this page was opened at.
 */
function publicOrigin(settings: InstanceSettings): string {
  const named = settings.public_url.effective;
  return (named || window.location.origin).replace(/\/+$/, "");
}

export function PrometheusRow({ values, settings, dirty, setField }: Omit<FormSectionProps, "section">) {
  const on = values.telemetryPrometheus;
  const url = `${publicOrigin(settings)}/metrics`;
  return (
    <SettingRow
      label="Expose /metrics"
      help="Same metrics as OTLP, no traces; anyone who reaches this instance reads it"
      source={
        settings.telemetry_prometheus
          ? telemetryFlagSourceNote(settings.telemetry_prometheus, "PDO_TELEMETRY_PROMETHEUS")
          : undefined
      }
      keywords="prometheus PDO_TELEMETRY_PROMETHEUS"
      dirty={dirty.has("telemetry-prometheus")}
      testId="setting-row-telemetry-prometheus"
    >
      <span className="set-mono set-faint set-ellipsis" title={url} data-testid="telemetry-metrics-url">
        {url}
      </span>
      <Switch label="Expose /metrics" checked={on} onChange={(next) => setField("telemetryPrometheus", next)} />
    </SettingRow>
  );
}

export function ActorRow({ values, settings, dirty, setField }: Omit<FormSectionProps, "section">) {
  const on = values.telemetryIncludeActor;
  return (
    <>
      <SwitchRow
        label="Include the Actor"
        help="Who launched, who answered: adds the person's identifier to the traces and the run metrics"
        source={
          settings.telemetry_include_actor
            ? telemetryFlagSourceNote(settings.telemetry_include_actor, "PDO_TELEMETRY_INCLUDE_ACTOR")
            : "Source: built-in default (off)."
        }
        keywords="identity PDO_TELEMETRY_INCLUDE_ACTOR"
        testId="setting-row-telemetry-include-actor"
        checked={on}
        onChange={(next) => setField("telemetryIncludeActor", next)}
        dirty={dirty.has("telemetry-include-actor")}
      />
      {/* Shown as soon as it is ticked, before the Save that switches it on. */}
      {on && (
        <div className="set-warn" role="alert" title={LEGAL} data-testid="telemetry-actor-warning">
          <ShieldAlert size={13} strokeWidth={1.5} aria-hidden />
          <span>Records each person's activity</span>
          <span className="set-faint set-ellipsis">inform · CSE · GDPR</span>
        </div>
      )}
    </>
  );
}
