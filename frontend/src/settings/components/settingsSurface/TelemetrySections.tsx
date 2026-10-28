/**
 * Settings › Telemetry (ADR-0090): the OTLP export — its switch, its collector, its
 * headers (saved with the form; a header's value is never shown again) and the answer of
 * its last export —, `/metrics` by its own switch, « include the Actor » with what it
 * obliges an employer to, then what is sent, read from the daemon's own list.
 */

import { Play, X } from "lucide-react";
import type { TelemetryStatus } from "../../../shared/generated/TelemetryStatus";
import type { SettingsSection, SettingsSectionId } from "../../model/settingsCategories";
import { telemetryEndpointSourceNote, telemetryOtlpSourceNote } from "../../model/sourceNotes";
import type { TelemetryHeaderDraft } from "../../model/telemetryDraft";
import { ago, useTelemetryStatus } from "../../store/useTelemetryStatus";
import { FieldBlock } from "./fields";
import type { FormSectionProps } from "./GeneralSections";
import Section from "./Section";
import { Identity, PrometheusMetrics, Switch } from "./TelemetryMetricsIdentity";

const INPUT =
  "rounded-md border bg-surface-2 px-2.5 py-1.5 font-mono text-text placeholder:text-text-3 focus:outline-none";

function border(dirty: boolean) {
  return dirty ? "border-st-await focus:border-st-await" : "border-line-strong focus:border-primary";
}

export default function TelemetrySections({
  sections,
  active,
  ...form
}: Omit<FormSectionProps, "section"> & { sections: SettingsSection[]; active: boolean }) {
  const telemetry = useTelemetryStatus(active);
  const section = (id: SettingsSectionId) => sections.find((s) => s.id === id)!;
  return (
    <>
      <Section section={section("otlp-export")}>
        <OtlpExport {...form} {...telemetry} />
      </Section>
      <Section section={section("prometheus-metrics")}>
        <PrometheusMetrics {...form} />
      </Section>
      <Section section={section("identity")}>
        <Identity {...form} />
      </Section>
      <Section section={section("what-is-sent")}>
        <WhatIsSent status={telemetry.status} />
      </Section>
    </>
  );
}

function OtlpExport({
  values,
  settings,
  dirty,
  setField,
  status,
  testing,
  testError,
  testExport,
  now,
}: Omit<FormSectionProps, "section"> & ReturnType<typeof useTelemetryStatus>) {
  const on = values.telemetryOtlp;
  const switchDirty = dirty.has("telemetry-otlp");
  const headers = values.telemetryHeaders;
  const setHeaders = (next: TelemetryHeaderDraft[]) => setField("telemetryHeaders", next);
  const storedNames = new Set((settings.telemetry_otlp_headers ?? []).map((h) => h.name));
  return (
    <div className="flex flex-col gap-4" style={{ maxWidth: 640 }}>
      <div className="flex flex-col gap-1" data-dirty={switchDirty ? "true" : undefined}>
        <Switch
          on={on}
          dirty={switchDirty}
          label="Export metrics and traces over OTLP"
          testId="setting-telemetry-otlp"
          onToggle={() => setField("telemetryOtlp", !on)}
        />
        <div className="pl-[34px] text-text-3" style={{ fontSize: "10.5px" }}>
          Off by default. Logs are never exported.
        </div>
        {settings.telemetry_otlp && (
          <div
            className="pl-[34px] text-text-3"
            style={{ fontSize: "10.5px" }}
            data-testid="setting-source-telemetry-otlp"
          >
            {telemetryOtlpSourceNote(settings.telemetry_otlp)}
          </div>
        )}
      </div>

      <FieldBlock
        label="Collector endpoint"
        htmlFor="setting-telemetry-endpoint"
        dirty={dirty.has("telemetry-endpoint")}
        help="OTLP over HTTP (protobuf). The path /v1/metrics and /v1/traces is added."
        source={
          settings.telemetry_otlp_endpoint
            ? telemetryEndpointSourceNote(settings.telemetry_otlp_endpoint)
            : undefined
        }
        sourceTestId="setting-source-telemetry-endpoint"
      >
        <input
          id="setting-telemetry-endpoint"
          data-testid="setting-telemetry-endpoint"
          type="url"
          spellCheck={false}
          placeholder="https://otel.example.org:4318"
          value={values.telemetryEndpoint}
          onChange={(e) => setField("telemetryEndpoint", e.target.value)}
          className={`${INPUT} w-full ${border(dirty.has("telemetry-endpoint"))}`}
          style={{ fontSize: "12px" }}
        />
      </FieldBlock>

      <FieldBlock
        label="Headers"
        testId="setting-telemetry-headers"
        dirty={dirty.has("telemetry-headers")}
        help="Sent with every export, e.g. an API key. Stored on the daemon; the value is never shown again."
      >
        <div className="flex flex-col gap-1.5">
          {headers.map((header, index) => (
            <div key={index} className="group flex items-center gap-1.5" data-testid="telemetry-header-row">
              <input
                aria-label="Header name"
                data-testid={`telemetry-header-name-${index}`}
                spellCheck={false}
                placeholder="Authorization"
                value={header.name}
                onChange={(e) =>
                  setHeaders(headers.map((h, i) => (i === index ? { ...h, name: e.target.value } : h)))
                }
                className={`${INPUT} w-[150px] ${border(dirty.has("telemetry-headers"))}`}
                style={{ fontSize: "12px" }}
              />
              <input
                aria-label="Header value"
                data-testid={`telemetry-header-value-${index}`}
                type="password"
                autoComplete="off"
                placeholder={
                  header.value === null && storedNames.has(header.name)
                    ? "•••••••••••• (set)"
                    : "value"
                }
                value={header.value ?? ""}
                onChange={(e) =>
                  setHeaders(
                    headers.map((h, i) =>
                      i === index ? { ...h, value: e.target.value === "" ? null : e.target.value } : h,
                    ),
                  )
                }
                className={`${INPUT} min-w-0 flex-1 ${border(dirty.has("telemetry-headers"))}`}
                style={{ fontSize: "12px" }}
              />
              <button
                type="button"
                aria-label={`Remove header ${header.name}`}
                data-testid={`telemetry-header-remove-${index}`}
                onClick={() => setHeaders(headers.filter((_, i) => i !== index))}
                className="rounded p-1 text-text-3 hover:bg-hover hover:text-text-2"
              >
                <X size={12} />
              </button>
            </div>
          ))}
          <button
            type="button"
            data-testid="telemetry-header-add"
            onClick={() => setHeaders([...headers, { name: "", value: null }])}
            className="self-start px-2.5 py-0.5 text-text-3 hover:text-text"
            style={{ fontSize: "11px" }}
          >
            + Add header
          </button>
        </div>
      </FieldBlock>

      <ExportStatusBar status={status} testing={testing} testError={testError} onTest={testExport} now={now} />
    </div>
  );
}

function ExportStatusBar({
  status,
  testing,
  testError,
  onTest,
  now,
}: {
  status: TelemetryStatus | null;
  testing: boolean;
  testError: string | null;
  onTest: () => Promise<void>;
  now: number;
}) {
  const last = status?.last_export ?? null;
  const exporting = status?.exporting ?? false;
  const [dot, text] = !exporting
    ? ["bg-text-3", "Export off: nothing leaves this instance."]
    : last
      ? [last.ok ? "bg-st-done" : "bg-st-failed", `Last export ${ago(last.at, now)} · ${last.detail}`]
      : ["bg-text-3", "No export yet."];
  return (
    <div className="flex flex-col gap-1">
      <div
        className="flex items-center gap-2.5 rounded-md border border-line bg-surface-1 px-3 py-2"
        data-testid="telemetry-export-status"
      >
        <span className={`h-1.5 w-1.5 shrink-0 rounded-full ${dot}`} />
        <span className="min-w-0 flex-1 truncate text-text-2" style={{ fontSize: "11px" }}>
          {text}
        </span>
        {status && (
          <span className="shrink-0 text-text-3" style={{ fontSize: "10.5px" }}>
            every {status.interval_secs} s
          </span>
        )}
        <button
          type="button"
          data-testid="telemetry-test-export"
          disabled={!exporting || testing}
          title={exporting ? undefined : "Turn the export on and save to send a test export."}
          onClick={() => void onTest()}
          className="flex shrink-0 items-center gap-1 rounded border border-line-strong bg-surface-2 px-2 py-1 text-text-2 hover:border-primary disabled:opacity-40"
          style={{ fontSize: "10.5px" }}
        >
          <Play size={9} />
          {testing ? "Sending…" : "Send a test export"}
        </button>
      </div>
      {testError && (
        <div className="text-st-failed" style={{ fontSize: "10.5px" }} data-testid="telemetry-test-error">
          {testError}
        </div>
      )}
    </div>
  );
}

function WhatIsSent({ status }: { status: TelemetryStatus | null }) {
  if (!status) {
    return (
      <div className="text-text-3" style={{ fontSize: "11px" }}>
        Loading…
      </div>
    );
  }
  const { sent } = status;
  const groups: [string, string[]][] = [
    ["On every measure", sent.resource],
    ["On the Run and node metrics", sent.run_attributes],
    ["On the traces only", sent.trace_only],
    ["Only with Include the Actor", sent.with_actor],
    ["Metrics", sent.metrics],
    ["Traces", sent.traces],
  ];
  return (
    <div className="flex flex-col gap-3" style={{ maxWidth: 640 }} data-testid="telemetry-what-is-sent">
      {groups.map(([label, items]) => (
        <div key={label} className="flex flex-col gap-1">
          <div className="font-medium text-text-2" style={{ fontSize: "11.5px" }}>
            {label}
          </div>
          <div className="flex flex-wrap gap-1">
            {items.map((item) => (
              <span
                key={item}
                className="rounded bg-surface-2 px-1.5 py-0.5 font-mono text-text-3"
                style={{ fontSize: "10.5px" }}
              >
                {item}
              </span>
            ))}
          </div>
        </div>
      ))}
      <ul className="flex list-disc flex-col gap-1 pl-4 text-text-3" style={{ fontSize: "10.5px" }}>
        <li>The repository is named by its remote URL, without credentials, or by its folder name. Never by a local path.</li>
        <li>The Run and node ids go on the traces only, never on the metrics.</li>
        <li>
          {status.include_actor
            ? "Who launched a Run, and who answered it, is sent: Include the Actor is on."
            : "Who launched a Run or answered it is never sent, unless you include the Actor."}
        </li>
      </ul>
    </div>
  );
}
