/**
 * Settings › Telemetry (ADR-0090), the sections beside the OTLP export: `/metrics`, read by
 * whoever reaches the instance, by its own switch; « include the Actor », with what it
 * obliges an employer to.
 */

import { TriangleAlert } from "lucide-react";
import type { InstanceSettings, StringSettingField } from "../../model/settings";
import { telemetryFlagSourceNote } from "../../model/sourceNotes";
import { CheckboxRow } from "./fields";
import type { FormSectionProps } from "./GeneralSections";

/** A switch with its label, the look of the OTLP export's and `/metrics`'. */
export function Switch({
  on,
  dirty,
  label,
  testId,
  onToggle,
}: {
  on: boolean;
  dirty: boolean;
  label: string;
  testId: string;
  onToggle: () => void;
}) {
  return (
    <div className="flex items-center gap-2.5">
      <button
        type="button"
        role="switch"
        aria-checked={on}
        aria-label={label}
        data-testid={testId}
        onClick={onToggle}
        className={`relative h-3.5 w-6 shrink-0 rounded-full transition-colors ${
          on ? "bg-acc" : "bg-fg-5"
        } ${dirty ? "outline outline-1 outline-offset-2 outline-st-await" : ""}`}
      >
        <span
          className={`absolute top-0.5 h-2.5 w-2.5 rounded-full bg-bg-1 transition-all ${
            on ? "left-3" : "left-0.5"
          }`}
        />
      </button>
      <span className="font-medium text-fg-2" style={{ fontSize: "11.5px" }}>
        {label}
      </span>
    </div>
  );
}

/**
 * The address people open PDO at: the Public URL when the daemon names one, else the
 * address this page was opened at.
 */
function publicOrigin(settings: InstanceSettings): string {
  const named = (settings as { public_url?: StringSettingField }).public_url?.effective;
  return (named || window.location.origin).replace(/\/+$/, "");
}

export function PrometheusMetrics({ values, settings, dirty, setField }: Omit<FormSectionProps, "section">) {
  const on = values.telemetryPrometheus;
  const switchDirty = dirty.has("telemetry-prometheus");
  const url = `${publicOrigin(settings)}/metrics`;
  return (
    <div className="flex flex-col gap-1" style={{ maxWidth: 640 }} data-dirty={switchDirty ? "true" : undefined}>
      <Switch
        on={on}
        dirty={switchDirty}
        label="Expose /metrics"
        testId="setting-telemetry-prometheus"
        onToggle={() => setField("telemetryPrometheus", !on)}
      />
      <div className="pl-[34px] text-fg-4" style={{ fontSize: "10.5px" }} data-testid="telemetry-metrics-url">
        {on ? "On: " : "Off. When on: "}
        <span className="font-mono">{url}</span>, same metrics as OTLP, no traces. Anyone who reaches PDO
        reads it.
      </div>
      {settings.telemetry_prometheus && (
        <div
          className="pl-[34px] text-fg-3"
          style={{ fontSize: "10.5px" }}
          data-testid="setting-source-telemetry-prometheus"
        >
          {telemetryFlagSourceNote(settings.telemetry_prometheus, "PDO_TELEMETRY_PROMETHEUS")}
        </div>
      )}
    </div>
  );
}

export function Identity({ values, settings, dirty, setField }: Omit<FormSectionProps, "section">) {
  const on = values.telemetryIncludeActor;
  return (
    <div className="flex flex-col gap-3" style={{ maxWidth: 640 }}>
      <CheckboxRow
        id="telemetry-include-actor"
        checked={on}
        onChange={(v) => setField("telemetryIncludeActor", v)}
        dirty={dirty.has("telemetry-include-actor")}
        label="Include the Actor (who launched, who answered)"
        help="Adds the person's identifier to traces and to the run metrics."
        source={
          settings.telemetry_include_actor
            ? telemetryFlagSourceNote(settings.telemetry_include_actor, "PDO_TELEMETRY_INCLUDE_ACTOR")
            : "Source: built-in default (off)."
        }
      />
      {on && <ActorWarning />}
    </div>
  );
}

/** ADR-0090 §4: what measuring each person's activity obliges an employer to. Shown as
 *  soon as the box is ticked, before the Save that switches it on. */
function ActorWarning() {
  return (
    <div
      role="alert"
      data-testid="telemetry-actor-warning"
      className="flex gap-2 rounded-md border border-st-await/40 bg-st-await/10 px-3 py-2.5"
    >
      <TriangleAlert size={12} className="mt-0.5 shrink-0 text-st-await" />
      <div className="flex flex-col gap-1">
        <div className="font-medium text-st-await" style={{ fontSize: "11px" }}>
          This turns the telemetry into a record of each person's activity.
        </div>
        <div className="text-fg-2" style={{ fontSize: "10.5px" }}>
          In France, before you switch it on: the people concerned must be informed beforehand
          (Code du travail, art. L1222-4), the CSE consulted (art. L2312-38), and the processing
          must have a GDPR basis and be in your record of processing. PDO cannot meet these
          obligations for you.
        </div>
      </div>
    </div>
  );
}
