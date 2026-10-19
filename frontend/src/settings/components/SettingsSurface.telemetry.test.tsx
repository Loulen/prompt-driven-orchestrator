import { render, screen, fireEvent, waitFor, within } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import type { InstanceSettings } from "../model/settings";
import type { TelemetryStatus } from "../../shared/generated/TelemetryStatus";

// ADR-0090 — Settings › Telemetry: the OTLP export, `/metrics`, « include the Actor ». Every api function the surface (and the panels it
// mounts) imports must exist in this factory: Vitest 4 wraps the module namespace in a
// Proxy that throws on first access of a missing key — see the note in
// `SettingsSurface.test.tsx`.
const fetchSettingsMock = vi.fn();
const updateSettingsMock = vi.fn();
const fetchUpdateStatusMock = vi.fn();
const fetchUpdateAttemptLogMock = vi.fn();
const checkForUpdateNowMock = vi.fn();
const fetchTelemetryStatusMock = vi.fn();
const sendTestExportMock = vi.fn();

vi.mock("../../skill/api/bank", () => ({
  fetchSkillBank: vi.fn().mockResolvedValue({ skills: [], folders: [], root_path: "/home/user/.pdo/skills" }),
  createSkill: vi.fn(),
  fetchSkill: vi.fn(),
  updateSkill: vi.fn(),
  deleteSkill: vi.fn(),
  fetchSkillReferents: vi.fn(),
  createSkillFolder: vi.fn(),
  updateSkillFolder: vi.fn(),
  deleteSkillFolder: vi.fn(),
}));

// The staging-profile panel reads the sandbox context's own API module.
vi.mock("../../sandbox/api/profiles", () => ({
  fetchSandboxProfiles: vi.fn().mockResolvedValue({ profiles: [], home: "/home/user" }),
  saveSandboxProfile: vi.fn(),
  deleteSandboxProfile: vi.fn(),
  fetchSandboxProfileReferents: vi.fn(),
}));

// #891: Settings › General › Stats absorptions reads the stats context's API.
vi.mock("../../stats/api/absorptions", () => ({
  fetchStatsAbsorptions: vi.fn().mockResolvedValue({ absorptions: [] }),
  uncombineStatsMember: vi.fn(),
}));

vi.mock("../api/settings", () => ({
  fetchSettings: (...args: unknown[]) => fetchSettingsMock(...args),
  updateSettings: (...args: unknown[]) => updateSettingsMock(...args),
}));
vi.mock("../api/agentProfiles", () => ({
  fetchAgentProfiles: vi.fn().mockResolvedValue({ profiles: [] }),
  createAgentProfile: vi.fn(),
  updateAgentProfile: vi.fn(),
  deleteAgentProfile: vi.fn(),
  fetchAgentProfileReferents: vi.fn(),
}));
vi.mock("../api/telemetry", () => ({
  fetchTelemetryStatus: (...args: unknown[]) => fetchTelemetryStatusMock(...args),
  sendTestExport: (...args: unknown[]) => sendTestExportMock(...args),
}));
vi.mock("../../platform/api/update", () => ({
  fetchUpdateStatus: (...args: unknown[]) => fetchUpdateStatusMock(...args),
  applyUpdate: vi.fn(),
  fetchUpdateAttemptLog: (...args: unknown[]) => fetchUpdateAttemptLogMock(...args),
  checkForUpdateNow: (...args: unknown[]) => checkForUpdateNowMock(...args),
}));

vi.mock("../../workspace/api/browse", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../workspace/api/browse")>()),
  browseFs: vi.fn(),
}));

vi.mock("../../workspace/api/provisioning", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../workspace/api/provisioning")>()),
  saveInstanceProvisioning: vi.fn().mockResolvedValue({ copy: [], hardlink: [], symlink: [] }),
  fetchInstanceProvisioning: vi.fn().mockResolvedValue({ copy: [], hardlink: [], symlink: [] }),
}));

import SettingsSurface from "./SettingsSurface";

function settings(extra: Partial<InstanceSettings> = {}): InstanceSettings {
  return {
    session_cap: { effective: 20, source: "default", stored: null, env: null, default: 20 },
    reaper_ttl_secs: { effective: 3600, source: "default", stored: null, env: null, default: 3600 },
    guard_timeout_secs: { effective: 60, source: "default", stored: null, env: null, default: 60 },
    max_attachments_mb: { effective: 50, source: "default", stored: null, env: null, default: 50 },
    default_model: { effective: null, source: "default", stored: null, env: null, default: null },
    default_harness: { effective: null, source: "default", stored: null, env: null, default: null },
    default_harness_model: { effective: {}, stored: {} },
    default_sandbox: { effective: "off", source: "default", stored: null, env: null, default: "off", reason: null },
    sandbox_docker: { available: true, reason: null, checked_at: "2026-07-01T10:00:00.000Z" },
    sandbox_profiles: [
      { name: "full", virtual: true },
      { name: "minimal", virtual: true },
    ],
    home: "/home/user",
    autocomplete_turn_end: { effective: false, source: "default", stored: null, env: null, default: false },
    default_auto_name: { effective: true, source: "default", stored: null, env: null, default: true },
    manager_enabled: { effective: false, source: "default", stored: null, env: null, default: false },
    review_agent_can_resolve: { effective: false, source: "default", stored: null, env: null, default: false },
    manager_profile: { effective: null, source: "default", stored: null, env: null, default: null },
    update_check: { effective: true, source: "default", stored: null, env: null, default: true },
    price_table: {
      manual_path: "/home/user/.pdo/prices/models.yaml",
      fetched_path: "/home/user/.pdo/prices/fetched.json",
      source: null,
      fetched_at: null,
      fetched_rows: 0,
      manual_keys: [],
      reason: null,
    },
    harness_descriptors: {
      path: "/home/user/.pdo/harnesses/descriptors.yaml",
      names: ["claude"],
      harnesses: [
        { name: "claude", source: "builtin", installed: true, models: ["sonnet"], efforts: [], has_effort: false, version: "1" },
      ],
      reason: null,
    },
    telemetry_otlp: { effective: false, source: "default", stored: null, env: null, default: false },
    telemetry_otlp_endpoint: { effective: null, source: "default", stored: null, env: null, default: null },
    telemetry_otlp_headers: [],
    telemetry_prometheus: { effective: false, source: "default", stored: null, env: null, default: false },
    telemetry_include_actor: { effective: false, source: "default", stored: null, env: null, default: false },
    public_url: { effective: null, source: "default", stored: null, env: null, default: null },
    updated_at: "2026-07-01T10:00:00.000Z",
    ...extra,
  } as unknown as InstanceSettings;
}

const SENT = {
  resource: ["service.name", "service.version", "service.instance.id"],
  run_attributes: ["pdo.project", "pdo.repository", "pdo.branch", "pdo.commit", "pdo.pipeline", "pdo.harness", "pdo.model"],
  trace_only: ["pdo.run.id", "pdo.node.id"],
  with_actor: ["pdo.actor"],
  metrics: ["pdo.runs.started"],
  traces: ["HTTP requests", "Run → node"],
};

function telemetry(overrides: Partial<TelemetryStatus> = {}): TelemetryStatus {
  return {
    exporting: false,
    metrics_endpoint: false,
    include_actor: false,
    interval_secs: 30,
    last_export: null,
    sent: SENT,
    ...overrides,
  };
}

function renderTelemetry() {
  return render(
    <SettingsSurface open onClose={() => {}} initialPosition={{ category: "telemetry" }} />,
  );
}

beforeEach(() => {
  fetchSettingsMock.mockReset().mockResolvedValue(settings());
  updateSettingsMock.mockReset().mockImplementation(async () => settings());
  fetchUpdateStatusMock.mockReset().mockResolvedValue(null);
  checkForUpdateNowMock.mockReset();
  fetchUpdateAttemptLogMock.mockReset();
  fetchTelemetryStatusMock.mockReset().mockResolvedValue(telemetry());
  sendTestExportMock.mockReset();
});

describe("Settings › Telemetry (ADR-0090)", () => {
  it("sits on the rail with its OTLP export and a read-only « What is sent »", async () => {
    renderTelemetry();
    expect(await screen.findByTestId("settings-category-telemetry")).toHaveTextContent("Telemetry");
    const otlp = await screen.findByTestId("settings-section-body-otlp-export");
    expect(within(otlp).getByText("OTLP export")).toBeInTheDocument();
    const sent = screen.getByTestId("settings-section-body-what-is-sent");
    expect(within(sent).getByText("read-only")).toBeInTheDocument();
    expect(await within(sent).findByText("pdo.repository")).toBeInTheDocument();
    expect(within(sent).getByText("pdo.run.id")).toBeInTheDocument();
  });

  it("is off by default: the switch is off and nothing leaves", async () => {
    renderTelemetry();
    const toggle = await screen.findByTestId("setting-telemetry-otlp");
    expect(toggle).toHaveAttribute("aria-checked", "false");
    expect(screen.getByTestId("setting-source-telemetry-otlp")).toHaveTextContent(
      "built-in default (off)",
    );
    expect(await screen.findByTestId("telemetry-export-status")).toHaveTextContent(
      "Export off: nothing leaves this instance.",
    );
    expect(screen.getByTestId("telemetry-test-export")).toBeDisabled();
  });

  it("saves the switch, the collector and a new header with the form", async () => {
    renderTelemetry();
    fireEvent.click(await screen.findByTestId("setting-telemetry-otlp"));
    fireEvent.change(screen.getByTestId("setting-telemetry-endpoint"), {
      target: { value: "https://otel.example.org:4318" },
    });
    fireEvent.click(screen.getByTestId("telemetry-header-add"));
    fireEvent.change(screen.getByTestId("telemetry-header-name-0"), {
      target: { value: "Authorization" },
    });
    fireEvent.change(screen.getByTestId("telemetry-header-value-0"), {
      target: { value: "Bearer s3cret" },
    });
    fireEvent.click(screen.getByTestId("settings-save"));
    await waitFor(() => expect(updateSettingsMock).toHaveBeenCalledTimes(1));
    expect(updateSettingsMock).toHaveBeenCalledWith({
      telemetry_otlp: true,
      telemetry_otlp_endpoint: "https://otel.example.org:4318",
      telemetry_otlp_headers: [{ name: "Authorization", value: "Bearer s3cret" }],
    });
  });

  it("shows a stored header as set, keeps it untouched, and removes it on demand", async () => {
    fetchSettingsMock.mockResolvedValue(
      settings({ telemetry_otlp_headers: [{ name: "Authorization", set: true }] }),
    );
    renderTelemetry();
    const value = await screen.findByTestId("telemetry-header-value-0");
    expect(value).toHaveValue("");
    expect(value).toHaveAttribute("placeholder", "•••••••••••• (set)");
    expect(screen.getByTestId("settings-save")).toBeDisabled();
    fireEvent.click(screen.getByTestId("telemetry-header-remove-0"));
    fireEvent.click(screen.getByTestId("settings-save"));
    await waitFor(() =>
      expect(updateSettingsMock).toHaveBeenCalledWith({ telemetry_otlp_headers: [] }),
    );
  });

  it("refuses a collector that is not an http address, before any write", async () => {
    renderTelemetry();
    fireEvent.change(await screen.findByTestId("setting-telemetry-endpoint"), {
      target: { value: "otel.example.org:4318" },
    });
    fireEvent.click(screen.getByTestId("settings-save"));
    expect(await screen.findByTestId("settings-error")).toHaveTextContent("http:// or https://");
    expect(updateSettingsMock).not.toHaveBeenCalled();
  });

  it("says the last export's answer and sends a test export", async () => {
    const at = new Date(Date.now() - 12_000).toISOString();
    fetchTelemetryStatusMock.mockResolvedValue(
      telemetry({
        exporting: true,
        last_export: { at, signal: "metrics", ok: true, detail: "200 OK" },
      }),
    );
    sendTestExportMock.mockResolvedValue(
      telemetry({
        exporting: true,
        last_export: { at: new Date().toISOString(), signal: "traces", ok: false, detail: "connection refused or unreachable" },
      }),
    );
    renderTelemetry();
    const bar = await screen.findByTestId("telemetry-export-status");
    await waitFor(() => expect(bar).toHaveTextContent(/Last export 1[0-9] s ago · 200 OK/));
    expect(bar).toHaveTextContent("every 30 s");
    fireEvent.click(screen.getByTestId("telemetry-test-export"));
    await waitFor(() => expect(sendTestExportMock).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(bar).toHaveTextContent("connection refused or unreachable"));
  });

  it("lists the /metrics and Identity sections between the export and « What is sent »", async () => {
    renderTelemetry();
    const sub = await screen.findByTestId("settings-section-body-prometheus-metrics");
    expect(within(sub).getByText("Prometheus /metrics")).toBeInTheDocument();
    expect(screen.getByTestId("settings-section-body-identity")).toHaveTextContent("Identity");
    const telemetrySections = new Set([
      "settings-section-body-otlp-export",
      "settings-section-body-prometheus-metrics",
      "settings-section-body-identity",
      "settings-section-body-what-is-sent",
    ]);
    const ids = Array.from(document.querySelectorAll("[data-testid^='settings-section-body-']"))
      .map((el) => el.getAttribute("data-testid"))
      .filter((id) => telemetrySections.has(id ?? ""));
    expect(ids).toEqual([
      "settings-section-body-otlp-export",
      "settings-section-body-prometheus-metrics",
      "settings-section-body-identity",
      "settings-section-body-what-is-sent",
    ]);
  });

  it("/metrics is off by default and names the address it would answer on", async () => {
    renderTelemetry();
    const toggle = await screen.findByTestId("setting-telemetry-prometheus");
    expect(toggle).toHaveAttribute("aria-checked", "false");
    expect(screen.getByTestId("telemetry-metrics-url")).toHaveTextContent(
      `Off. When on: ${window.location.origin}/metrics, same metrics as OTLP, no traces.`,
    );
    expect(screen.getByTestId("setting-source-telemetry-prometheus")).toHaveTextContent(
      "built-in default (off)",
    );
  });

  it("names the /metrics address with the Public URL when the daemon gives one", async () => {
    fetchSettingsMock.mockResolvedValue(
      settings({
        public_url: { effective: "https://pdo.example.org", source: "stored", stored: "https://pdo.example.org", env: null, default: null },
      }),
    );
    renderTelemetry();
    expect(await screen.findByTestId("telemetry-metrics-url")).toHaveTextContent(
      "https://pdo.example.org/metrics",
    );
  });

  it("saves the /metrics switch on its own", async () => {
    renderTelemetry();
    fireEvent.click(await screen.findByTestId("setting-telemetry-prometheus"));
    expect(screen.getByTestId("telemetry-metrics-url")).toHaveTextContent(
      `On: ${window.location.origin}/metrics, same metrics as OTLP`,
    );
    fireEvent.click(screen.getByTestId("settings-save"));
    await waitFor(() =>
      expect(updateSettingsMock).toHaveBeenCalledWith({ telemetry_prometheus: true }),
    );
  });

  it("« Include the Actor » is off by default; ticked, the warning shows before the save", async () => {
    renderTelemetry();
    const box = await screen.findByTestId("setting-telemetry-include-actor");
    expect(box).not.toBeChecked();
    expect(screen.queryByTestId("telemetry-actor-warning")).not.toBeInTheDocument();
    fireEvent.click(box);
    const warning = screen.getByTestId("telemetry-actor-warning");
    expect(warning).toHaveTextContent("This turns the telemetry into a record of each person's activity.");
    expect(warning).toHaveTextContent("L1222-4");
    expect(warning).toHaveTextContent("L2312-38");
    expect(warning).toHaveTextContent("GDPR");
    expect(updateSettingsMock).not.toHaveBeenCalled();
    fireEvent.click(screen.getByTestId("settings-save"));
    await waitFor(() =>
      expect(updateSettingsMock).toHaveBeenCalledWith({ telemetry_include_actor: true }),
    );
  });

  it("says in « What is sent » when the Actor goes with the measures", async () => {
    fetchTelemetryStatusMock.mockResolvedValue(telemetry({ include_actor: true }));
    renderTelemetry();
    const sent = await screen.findByTestId("telemetry-what-is-sent");
    expect(within(sent).getByText("pdo.actor")).toBeInTheDocument();
    expect(sent).toHaveTextContent("Include the Actor is on");
  });
});
