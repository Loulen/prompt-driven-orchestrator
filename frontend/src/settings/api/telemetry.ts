import type { TelemetryStatus } from "../../shared/generated/TelemetryStatus";
import { request } from "../../shared/http/client";

/** Whether the OTLP export runs, its last export's answer, and what is sent (ADR-0090). */
export function fetchTelemetryStatus(): Promise<TelemetryStatus> {
  return request<TelemetryStatus>("GET", "/telemetry/status");
}

/** Export now, a test span included; `409` while the export is off. */
export function sendTestExport(): Promise<TelemetryStatus> {
  return request<TelemetryStatus>("POST", "/telemetry/test-export");
}
