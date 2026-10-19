/**
 * The Telemetry part of the instance form (ADR-0090): the OTLP switch, the collector's
 * address and the headers. A header's value is a secret the daemon never sends back, so a
 * stored header is drafted without its value (`null`: keep it) until someone types a new one.
 */

import type { OtlpHeaderEdit } from "../../shared/generated/OtlpHeaderEdit";
import type { UpdateSettingsRequest } from "../../shared/generated/UpdateSettingsRequest";
import type { InstanceSettings } from "./settings";
import type { SettingsFieldId } from "./settingsCategories";

export interface TelemetryHeaderDraft {
  name: string;
  /** `null`: the value stored under this name is kept. */
  value: string | null;
}

export interface TelemetryDraft {
  telemetryOtlp: boolean;
  telemetryEndpoint: string;
  telemetryHeaders: TelemetryHeaderDraft[];
}

export function seedTelemetry(settings: InstanceSettings): TelemetryDraft {
  return {
    telemetryOtlp: settings.telemetry_otlp?.effective ?? false,
    telemetryEndpoint: settings.telemetry_otlp_endpoint?.effective ?? "",
    telemetryHeaders: (settings.telemetry_otlp_headers ?? []).map((header) => ({
      name: header.name,
      value: null,
    })),
  };
}

/** The rows that say something: a blank row (no name, no value) is not a header. */
function meaningful(rows: TelemetryHeaderDraft[]): TelemetryHeaderDraft[] {
  return rows
    .map((row) => ({ name: row.name.trim(), value: row.value === "" ? null : row.value }))
    .filter((row) => row.name !== "" || row.value !== null);
}

function storedNames(settings: InstanceSettings): string[] {
  return (settings.telemetry_otlp_headers ?? []).map((header) => header.name);
}

function headersDirty(rows: TelemetryHeaderDraft[], settings: InstanceSettings): boolean {
  const stored = storedNames(settings);
  const rowsNow = meaningful(rows);
  return (
    rowsNow.length !== stored.length ||
    rowsNow.some((row, index) => row.name !== stored[index] || row.value !== null)
  );
}

export function telemetryDirty(values: TelemetryDraft, settings: InstanceSettings): SettingsFieldId[] {
  const dirty: SettingsFieldId[] = [];
  if (values.telemetryOtlp !== (settings.telemetry_otlp?.effective ?? false)) {
    dirty.push("telemetry-otlp");
  }
  if (values.telemetryEndpoint.trim() !== (settings.telemetry_otlp_endpoint?.effective ?? "")) {
    dirty.push("telemetry-endpoint");
  }
  if (headersDirty(values.telemetryHeaders, settings)) dirty.push("telemetry-headers");
  return dirty;
}

const HEADER_NAME = /^[!#$%&'*+\-.^_`|~0-9A-Za-z]+$/;

/** The Telemetry keys of the PUT payload, or the first invalid field named. */
export function telemetryPatch(
  values: TelemetryDraft,
  settings: InstanceSettings,
): { patch: UpdateSettingsRequest } | { error: string } {
  const patch: UpdateSettingsRequest = {};
  const dirty = new Set(telemetryDirty(values, settings));
  if (dirty.has("telemetry-otlp")) patch.telemetry_otlp = values.telemetryOtlp;
  if (dirty.has("telemetry-endpoint")) {
    const endpoint = values.telemetryEndpoint.trim();
    if (endpoint !== "" && !/^https?:\/\/[^\s/?#]+/.test(endpoint)) {
      return { error: "The collector endpoint must be an http:// or https:// address." };
    }
    // `""` clears the stored address.
    patch.telemetry_otlp_endpoint = endpoint;
  }
  if (dirty.has("telemetry-headers")) {
    const stored = storedNames(settings).map((name) => name.toLowerCase());
    const headers: OtlpHeaderEdit[] = [];
    for (const row of meaningful(values.telemetryHeaders)) {
      if (!HEADER_NAME.test(row.name)) {
        return { error: `\`${row.name || "(empty)"}\` is not a valid header name.` };
      }
      if (row.value === null && !stored.includes(row.name.toLowerCase())) {
        return { error: `The header \`${row.name}\` needs a value.` };
      }
      headers.push({ name: row.name, value: row.value });
    }
    patch.telemetry_otlp_headers = headers;
  }
  return { patch };
}
