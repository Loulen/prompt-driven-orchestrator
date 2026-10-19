import { describe, expect, it } from "vitest";
import type { InstanceSettings } from "./settings";
import { seedTelemetry, telemetryDirty, telemetryPatch } from "./telemetryDraft";

function settings(headers: string[] = []): InstanceSettings {
  return {
    telemetry_otlp: { effective: true, source: "stored", stored: true, env: null, default: false },
    telemetry_otlp_endpoint: {
      effective: "http://127.0.0.1:4318",
      source: "stored",
      stored: "http://127.0.0.1:4318",
      env: null,
      default: null,
    },
    telemetry_otlp_headers: headers.map((name) => ({ name, set: true })),
  } as unknown as InstanceSettings;
}

describe("the Telemetry draft (ADR-0090)", () => {
  it("seeds a stored header without its value and is clean", () => {
    const s = settings(["Authorization"]);
    const draft = seedTelemetry(s);
    expect(draft.telemetryHeaders).toEqual([{ name: "Authorization", value: null }]);
    expect(telemetryDirty(draft, s)).toEqual([]);
  });

  it("a blank row is not a header", () => {
    const s = settings();
    const draft = { ...seedTelemetry(s), telemetryHeaders: [{ name: " ", value: null }] };
    expect(telemetryDirty(draft, s)).toEqual([]);
  });

  it("a stored header renamed needs a value, a kept one sends none", () => {
    const s = settings(["Authorization"]);
    const renamed = { ...seedTelemetry(s), telemetryHeaders: [{ name: "X-Key", value: null }] };
    expect(telemetryPatch(renamed, s)).toEqual({ error: "The header `X-Key` needs a value." });
    const added = {
      ...seedTelemetry(s),
      telemetryHeaders: [
        { name: "Authorization", value: null },
        { name: "X-Team", value: "core" },
      ],
    };
    expect(telemetryPatch(added, s)).toEqual({
      patch: {
        telemetry_otlp_headers: [
          { name: "Authorization", value: null },
          { name: "X-Team", value: "core" },
        ],
      },
    });
  });

  it("clearing the collector sends the empty string", () => {
    const s = settings();
    const draft = { ...seedTelemetry(s), telemetryEndpoint: "  " };
    expect(telemetryPatch(draft, s)).toEqual({ patch: { telemetry_otlp_endpoint: "" } });
  });

  it("refuses an invalid header name", () => {
    const s = settings();
    const draft = { ...seedTelemetry(s), telemetryHeaders: [{ name: "Bad Name", value: "v" }] };
    expect(telemetryPatch(draft, s)).toEqual({ error: "`Bad Name` is not a valid header name." });
  });
});
