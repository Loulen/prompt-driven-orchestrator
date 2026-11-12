/**
 * CORE-248: Settings without the right to change the instance settings. Every field of
 * the instance categories is greyed, the refusal's sentence in its tooltip; nothing can
 * become dirty, so the Save bar never shows. No banner. Your account and this device stay
 * editable. A caller who may not even read the settings reads the sentence instead.
 */
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Decision, OperationQuestion } from "../../shared/authorization/api";
import { ApiError } from "../../shared/http/client";
import type { InstanceSettings } from "../model/settings";
import { rowOf, switchOf } from "./settingsSurface/testing";

// Every api function the surface (and the panels it mounts) imports must exist in these
// factories — see the note in `SettingsSurface.test.tsx`.
const fetchSettingsMock = vi.fn();
const updateSettingsMock = vi.fn();
const fetchAgentProfilesMock = vi.fn();
const fetchSkillBankMock = vi.fn();

vi.mock("../../shared/authorization/api", async (importOriginal) => ({
  ...(await importOriginal<object>()),
  askAuthorization: vi.fn(),
}));
vi.mock("../../skill/api/bank", () => ({
  fetchSkillBank: (...args: unknown[]) => fetchSkillBankMock(...args),
  createSkill: vi.fn(),
  fetchSkill: vi.fn(),
  updateSkill: vi.fn(),
  deleteSkill: vi.fn(),
  fetchSkillReferents: vi.fn(),
  createSkillFolder: vi.fn(),
  updateSkillFolder: vi.fn(),
  deleteSkillFolder: vi.fn(),
}));
vi.mock("../../sandbox/api/profiles", () => ({
  fetchSandboxProfiles: vi.fn().mockResolvedValue({ profiles: [], home: "/home/user" }),
  saveSandboxProfile: vi.fn(),
  deleteSandboxProfile: vi.fn(),
  fetchSandboxProfileReferents: vi.fn(),
}));
vi.mock("../../stats/api/absorptions", () => ({
  fetchStatsAbsorptions: vi.fn().mockResolvedValue({ absorptions: [] }),
  uncombineStatsMember: vi.fn(),
}));
vi.mock("../api/settings", () => ({
  fetchSettings: (...args: unknown[]) => fetchSettingsMock(...args),
  updateSettings: (...args: unknown[]) => updateSettingsMock(...args),
}));
vi.mock("../api/agentProfiles", () => ({
  fetchAgentProfiles: (...args: unknown[]) => fetchAgentProfilesMock(...args),
  createAgentProfile: vi.fn(),
  updateAgentProfile: vi.fn(),
  deleteAgentProfile: vi.fn(),
  fetchAgentProfileReferents: vi.fn(),
}));
vi.mock("../api/telemetry", () => ({
  fetchTelemetryStatus: vi.fn(() => new Promise(() => {})),
  sendTestExport: vi.fn(),
}));
vi.mock("../../platform/api/update", () => ({
  fetchUpdateStatus: vi.fn().mockResolvedValue(null),
  applyUpdate: vi.fn(),
  fetchUpdateAttemptLog: vi.fn(),
  checkForUpdateNow: vi.fn(),
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

const { askAuthorization } = await import("../../shared/authorization/api");
const { default: SettingsSurface } = await import("./SettingsSurface");

// Every case renders the whole surface: give the file its own ceiling.
vi.setConfig({ testTimeout: 120_000 });

const WHY = "Changing the instance settings needs an administrator.";
const READ_WHY = "You can't see this instance's settings. Ask an administrator for an instance role.";

function refusal(sentence: string): ApiError {
  return new ApiError(sentence, {
    status: 403,
    body: { error: "operation_refused", message: "operation refused: no", reason: "no", sentence },
  });
}

/** `instance_settings` × `update` refused (or not), every other Operation allowed. */
function answer(updateRefused: boolean) {
  vi.mocked(askAuthorization).mockImplementation(async (questions: OperationQuestion[]) =>
    questions.map(
      (q): Decision =>
        updateRefused && q.resource === "instance_settings" && q.verb === "update"
          ? { decision: "deny", reason: "the test rules", sentence: WHY }
          : { decision: "allow" },
    ),
  );
}

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

beforeEach(() => {
  vi.clearAllMocks();
  fetchSettingsMock.mockReset().mockResolvedValue(settings());
  updateSettingsMock.mockReset().mockImplementation(async () => settings());
  fetchAgentProfilesMock.mockReset().mockResolvedValue({ profiles: [] });
  fetchSkillBankMock
    .mockReset()
    .mockResolvedValue({ skills: [], folders: [], root_path: "/home/user/.pdo/skills" });
});

const general = () => render(<SettingsSurface open onClose={() => {}} initialPosition={{ category: "general" }} />);

describe("Settings without `instance_settings` × `update` (CORE-248)", () => {
  it("greys every field of the instance categories, the sentence in its tooltip, and never shows Save", async () => {
    answer(true);
    general();
    await waitFor(() => expect(screen.getByTestId("setting-session-cap")).toBeDisabled());
    expect(vi.mocked(askAuthorization).mock.calls.flat(1)).toContainEqual([
      { resource: "instance_settings", verb: "update" },
    ]);
    expect(switchOf("default-auto-name")).toBeDisabled();
    expect(switchOf("review-agent-can-resolve")).toBeDisabled();
    expect(screen.getByTestId("setting-reaper-ttl")).toBeDisabled();
    // The sentence on hover, over the greyed control.
    const locked = screen.getByTestId("setting-row-default-auto-name-locked");
    fireEvent.pointerEnter(locked);
    fireEvent.pointerMove(locked);
    expect(await screen.findByRole("tooltip")).toHaveTextContent(WHY);
    // Nothing becomes dirty: no Save bar, no amber edge.
    fireEvent.change(screen.getByTestId("setting-session-cap"), { target: { value: "3" } });
    fireEvent.click(switchOf("default-auto-name"));
    expect(screen.queryByTestId("settings-footer")).not.toBeInTheDocument();
    expect(rowOf("session-cap")).not.toHaveAttribute("data-dirty");
    expect(updateSettingsMock).not.toHaveBeenCalled();
    // No banner: the grey says it.
    expect(document.querySelector(".d-banner")).toBeNull();
  });

  it("leaves Your account and This device editable", async () => {
    answer(true);
    render(<SettingsSurface open onClose={() => {}} initialPosition={{ category: "appearance" }} />);
    await waitFor(() => expect(askAuthorization).toHaveBeenCalled());
    await screen.findByTestId("setting-row-dark-mode");
    await waitFor(() => expect(screen.getByTestId("setting-session-cap")).toBeDisabled());
    expect(within(rowOf("dark-mode")).getByRole("switch")).toBeEnabled();
    expect(screen.queryByTestId("setting-row-dark-mode-locked")).not.toBeInTheDocument();
  });

  it("leaves the fields editable for a caller allowed to change the settings", async () => {
    answer(false);
    general();
    await waitFor(() => expect(askAuthorization).toHaveBeenCalled());
    const cap = await screen.findByTestId("setting-session-cap");
    expect(cap).toBeEnabled();
    expect(screen.queryByTestId("setting-row-session-cap-locked")).not.toBeInTheDocument();
    fireEvent.change(cap, { target: { value: "3" } });
    expect(await screen.findByTestId("settings-footer")).toBeInTheDocument();
  });

  it("says the sentence when the caller may not read the settings, and does not crash", async () => {
    answer(true);
    fetchSettingsMock.mockReset().mockRejectedValue(refusal(READ_WHY));
    general();
    expect((await screen.findAllByTestId("settings-refused"))[0]).toHaveTextContent(READ_WHY);
    expect(screen.queryByTestId("settings-footer")).not.toBeInTheDocument();
  });

  it("shows the refusal of the agent profiles and of the skills, with no New profile", async () => {
    answer(false);
    fetchAgentProfilesMock.mockReset().mockRejectedValue(refusal("You can't see the agent profiles."));
    fetchSkillBankMock.mockReset().mockRejectedValue(refusal("You can't see the skills."));
    render(<SettingsSurface open onClose={() => {}} initialPosition={{ category: "agents" }} />);
    expect(await screen.findByTestId("agent-profiles-refused")).toHaveTextContent("You can't see the agent profiles.");
    expect(screen.queryByTestId("agent-profile-new")).not.toBeInTheDocument();
    fireEvent.click(screen.getByTestId("setting-open-skill-bank"));
    expect(await screen.findByTestId("skill-bank-refused")).toHaveTextContent("You can't see the skills.");
  });
});
