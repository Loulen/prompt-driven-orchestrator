/**
 * Settings › Telemetry (ADR-0090): the OTLP export — its switch, its collector, its
 * headers (saved with the form; a header's value is never shown again) and, under them,
 * the status line of its last export —, `/metrics` by its own switch, « include the
 * Actor » with what it obliges an employer to, then what is sent, read from the daemon's
 * own list.
 */

import { Loader2, Play, Plus, X } from "lucide-react";
import type { TelemetryStatus } from "../../../shared/generated/TelemetryStatus";
import { Button, EmptyState, IconButton, TextInput } from "../../../shared/ui/kit";
import type { SettingsSection, SettingsSectionId } from "../../model/settingsCategories";
import { telemetryEndpointSourceNote, telemetryOtlpSourceNote } from "../../model/sourceNotes";
import type { TelemetryHeaderDraft } from "../../model/telemetryDraft";
import { ago, useTelemetryStatus } from "../../store/useTelemetryStatus";
import { SettingRow, SwitchRow } from "./fields";
import type { FormSectionProps } from "./GeneralSections";
import Section, { SectionExtra } from "./Section";
import { ActorRow, PrometheusRow } from "./TelemetryMetricsIdentity";

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
        <OtlpRows {...form} />
        <SectionExtra>
          <ExportStatus
            status={telemetry.status}
            testing={telemetry.testing}
            testError={telemetry.testError}
            onTest={telemetry.testExport}
            now={telemetry.now}
          />
        </SectionExtra>
      </Section>
      <Section section={section("prometheus-metrics")}>
        <PrometheusRow {...form} />
      </Section>
      <Section section={section("identity")}>
        <ActorRow {...form} />
      </Section>
      <Section section={section("what-is-sent")}>
        <SectionExtra>
          <WhatIsSent status={telemetry.status} />
        </SectionExtra>
      </Section>
    </>
  );
}

function OtlpRows({ values, settings, dirty, setField }: Omit<FormSectionProps, "section">) {
  const headers = values.telemetryHeaders;
  const setHeaders = (next: TelemetryHeaderDraft[]) => setField("telemetryHeaders", next);
  const storedNames = new Set((settings.telemetry_otlp_headers ?? []).map((h) => h.name));
  return (
    <>
      <SwitchRow
        label="Export over OTLP"
        help="Metrics and traces; off by default; logs are never exported"
        source={settings.telemetry_otlp ? telemetryOtlpSourceNote(settings.telemetry_otlp) : undefined}
        keywords="PDO_TELEMETRY_OTLP opentelemetry"
        testId="setting-row-telemetry-otlp"
        checked={values.telemetryOtlp}
        onChange={(on) => setField("telemetryOtlp", on)}
        dirty={dirty.has("telemetry-otlp")}
      />
      <SettingRow
        label="Collector endpoint"
        help="OTLP over HTTP (protobuf); /v1/metrics and /v1/traces are added"
        source={
          settings.telemetry_otlp_endpoint
            ? telemetryEndpointSourceNote(settings.telemetry_otlp_endpoint)
            : undefined
        }
        keywords="PDO_TELEMETRY_OTLP_ENDPOINT"
        kind="wide"
        dirty={dirty.has("telemetry-endpoint")}
        htmlFor="setting-telemetry-endpoint"
        testId="setting-row-telemetry-endpoint"
      >
        <TextInput
          id="setting-telemetry-endpoint"
          data-testid="setting-telemetry-endpoint"
          mono
          type="url"
          spellCheck={false}
          placeholder="https://otel.example.org:4318"
          value={values.telemetryEndpoint}
          onChange={(e) => setField("telemetryEndpoint", e.target.value)}
        />
      </SettingRow>
      <SettingRow
        label="Headers"
        help="Sent with every export (an API key); stored on the daemon, the value is never shown again"
        kind="wide"
        top
        dirty={dirty.has("telemetry-headers")}
        testId="setting-row-telemetry-headers"
      >
        <div className="set-headers">
          {headers.map((header, index) => (
            <div key={index} className="set-line" data-testid="telemetry-header-row">
              <TextInput
                mono
                className="set-headers__name"
                aria-label="Header name"
                data-testid={`telemetry-header-name-${index}`}
                spellCheck={false}
                placeholder="Authorization"
                value={header.name}
                onChange={(e) =>
                  setHeaders(headers.map((h, i) => (i === index ? { ...h, name: e.target.value } : h)))
                }
              />
              <TextInput
                mono
                aria-label="Header value"
                data-testid={`telemetry-header-value-${index}`}
                type="password"
                autoComplete="off"
                placeholder={header.value === null && storedNames.has(header.name) ? "•••••••• set" : "value"}
                value={header.value ?? ""}
                onChange={(e) =>
                  setHeaders(
                    headers.map((h, i) =>
                      i === index ? { ...h, value: e.target.value === "" ? null : e.target.value } : h,
                    ),
                  )
                }
              />
              <IconButton
                icon={X}
                label={`Remove header ${header.name}`}
                size="sm"
                data-testid={`telemetry-header-remove-${index}`}
                onClick={() => setHeaders(headers.filter((_, i) => i !== index))}
              />
            </div>
          ))}
          <span>
            <Button
              size="sm"
              tone="ghost"
              icon={Plus}
              data-testid="telemetry-header-add"
              onClick={() => setHeaders([...headers, { name: "", value: null }])}
            >
              Header
            </Button>
          </span>
        </div>
      </SettingRow>
    </>
  );
}

/** The export's status line: dot, last export (or why nothing leaves), interval, Test. */
function ExportStatus({
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
  const [st, text, tip] = !exporting
    ? ["skipped", "Export off", "Nothing leaves this instance"]
    : last
      ? [last.ok ? "done" : "failed", `Last export ${ago(last.at, now)} · ${last.detail}`, undefined]
      : ["skipped", "No export yet", undefined];
  return (
    <>
      <div className="set-tstat" data-st={st} data-testid="telemetry-export-status">
        <span className="set-dot" aria-hidden />
        <span className="set-ellipsis" title={tip ?? text}>
          {text}
        </span>
        {status && exporting && <span className="set-faint set-mono text-xs">every {status.interval_secs} s</span>}
        <span className="d-grow" />
        <Button
          size="sm"
          icon={Play}
          data-testid="telemetry-test-export"
          disabled={!exporting || testing}
          title={exporting ? "Send a test export" : "Turn the export on and save first"}
          onClick={() => void onTest()}
        >
          {testing ? "Sending…" : "Test"}
        </Button>
      </div>
      {testError && (
        <div className="set-row__note set-row__note--failed" role="alert" data-testid="telemetry-test-error">
          <span className="set-ellipsis" title={testError}>
            {testError}
          </span>
        </div>
      )}
    </>
  );
}

function WhatIsSent({ status }: { status: TelemetryStatus | null }) {
  if (!status) {
    return (
      <EmptyState icon={Loader2} spin layout="row">
        Loading
      </EmptyState>
    );
  }
  const { sent } = status;
  const groups: [string, string[], string?][] = [
    ["Every measure", sent.resource],
    [
      "Run and node metrics",
      sent.run_attributes,
      "The repository is its remote URL without credentials, or its folder name; never a local path",
    ],
    ["Traces only", sent.trace_only, "Run and node ids go on the traces, never on the metrics"],
    [
      "With the Actor",
      sent.with_actor,
      status.include_actor ? "Sent: Include the Actor is on" : "Never sent unless you include the Actor",
    ],
    ["Metrics", sent.metrics],
    ["Traces", sent.traces],
  ];
  return (
    <dl className="set-sent" data-testid="telemetry-what-is-sent">
      {groups.map(([label, items, tip]) => (
        <div key={label} className="contents">
          <dt title={tip}>
            {label}
            {label === "With the Actor" && (
              <span className={`d-chip ${status.include_actor ? "text-st-await" : ""}`}>
                {status.include_actor ? "on" : "off"}
              </span>
            )}
          </dt>
          <dd>
            {items.map((item) => (
              <code key={item} className="d-chip d-chip--mono">
                {item}
              </code>
            ))}
          </dd>
        </div>
      ))}
    </dl>
  );
}
