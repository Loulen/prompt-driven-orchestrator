import { render, screen, fireEvent, waitFor, within } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import type { InstanceSettings, UpdateStatus } from "../types";

// #697 — Settings › General › Version & update. Every api function the surface (and the
// panels it mounts) imports must exist in this factory: Vitest 4 wraps the module
// namespace in a Proxy that throws on first access of a missing key — see the note in
// `SettingsSurface.test.tsx`.
const fetchSettingsMock = vi.fn();
const updateSettingsMock = vi.fn();
const fetchUpdateStatusMock = vi.fn();
const checkForUpdateNowMock = vi.fn();

vi.mock("../api", () => ({
  fetchAgentProfiles: vi.fn().mockResolvedValue({ profiles: [] }),
  createAgentProfile: vi.fn(),
  updateAgentProfile: vi.fn(),
  deleteAgentProfile: vi.fn(),
  fetchAgentProfileReferents: vi.fn(),
  saveInstanceProvisioning: vi.fn().mockResolvedValue({ copy: [], hardlink: [], symlink: [] }),
  fetchSettings: (...args: unknown[]) => fetchSettingsMock(...args),
  updateSettings: (...args: unknown[]) => updateSettingsMock(...args),
  browseFs: vi.fn(),
  fetchSandboxProfiles: vi.fn().mockResolvedValue({ profiles: [], home: "/home/user" }),
  saveSandboxProfile: vi.fn(),
  deleteSandboxProfile: vi.fn(),
  fetchSandboxProfileReferents: vi.fn(),
  fetchInstanceProvisioning: vi.fn().mockResolvedValue({ copy: [], hardlink: [], symlink: [] }),
  fetchSkillBank: vi.fn().mockResolvedValue({ skills: [], folders: [], root_path: "/home/user/.pdo/skills" }),
  createSkill: vi.fn(),
  fetchSkill: vi.fn(),
  updateSkill: vi.fn(),
  deleteSkill: vi.fn(),
  fetchSkillReferents: vi.fn(),
  createSkillFolder: vi.fn(),
  updateSkillFolder: vi.fn(),
  deleteSkillFolder: vi.fn(),
  fetchUpdateStatus: (...args: unknown[]) => fetchUpdateStatusMock(...args),
  checkForUpdateNow: (...args: unknown[]) => checkForUpdateNowMock(...args),
  ApiError: class ApiError extends Error {
    status?: number;
    body?: unknown;
  },
}));

import SettingsSurface from "./SettingsSurface";
import { UPDATE_STATUS_CHANGED } from "../lib/updateStatus";

function settings(): InstanceSettings {
  return {
    session_cap: { effective: 20, source: "default", stored: null, env: null, default: 20 },
    reaper_ttl_secs: { effective: 3600, source: "default", stored: null, env: null, default: 3600 },
    guard_timeout_secs: { effective: 60, source: "default", stored: null, env: null, default: 60 },
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
    updated_at: "2026-07-01T10:00:00.000Z",
  } as unknown as InstanceSettings;
}

function status(overrides: Partial<UpdateStatus> = {}): UpdateStatus {
  return {
    installed_version: "1.58.1",
    latest_version: "1.59.0",
    newer_available: true,
    checked_at: new Date(Date.now() - 2 * 3600_000).toISOString(),
    source: "GitHub Releases",
    source_url: "https://api.github.com/repos/Loulen/prompt-driven-orchestrator/releases/latest",
    check_enabled: true,
    install_method: "homebrew",
    manual_command: "brew update && brew upgrade Loulen/tap/pdo",
    supervision: "systemd",
    reason: null,
    last_error: null,
    ...overrides,
  };
}

function renderVersionSection() {
  return render(
    <SettingsSurface
      open
      onClose={() => {}}
      initialPosition={{ category: "general", section: "version-update" }}
    />,
  );
}

beforeEach(() => {
  fetchSettingsMock.mockReset().mockResolvedValue(settings());
  updateSettingsMock.mockReset();
  fetchUpdateStatusMock.mockReset();
  checkForUpdateNowMock.mockReset();
});

describe("Settings › General › Version & update (#697)", () => {
  it("is the fourth section of General, saves as you go, and reads GET /update once on open", async () => {
    fetchUpdateStatusMock.mockResolvedValue(status());
    renderVersionSection();

    const body = await screen.findByTestId("settings-section-body-version-update");
    expect(within(body).getByText("Version & update")).toBeInTheDocument();
    expect(within(body).getByText("saves as you go")).toBeInTheDocument();
    // Sub-column entry, after Runs.
    const rail = screen.getByTestId("settings-section-version-update");
    expect(rail).toBeInTheDocument();
    await waitFor(() => expect(fetchUpdateStatusMock).toHaveBeenCalledTimes(1));
    // Reading the cache never triggers a check.
    expect(checkForUpdateNowMock).not.toHaveBeenCalled();
  });

  it("shows installed, latest (amber when newer), last check with source, and install method", async () => {
    fetchUpdateStatusMock.mockResolvedValue(status());
    renderVersionSection();

    expect(await screen.findByTestId("setting-version-installed")).toHaveTextContent("v1.58.1");
    const latest = screen.getByTestId("setting-version-latest");
    expect(latest).toHaveTextContent("v1.59.0");
    expect(within(latest).getByText("v1.59.0").className).toContain("text-st-await");
    expect(screen.queryByTestId("setting-version-uptodate")).not.toBeInTheDocument();
    const checked = screen.getByTestId("setting-version-checked-at");
    expect(checked).toHaveTextContent("2 h ago");
    expect(checked).toHaveTextContent("GitHub Releases");
    expect(screen.getByTestId("setting-version-install-method")).toHaveTextContent(
      "Homebrew · systemd service",
    );
    const cmd = screen.getByTestId("setting-version-manual-command");
    expect(cmd).toHaveTextContent("brew update && brew upgrade Loulen/tap/pdo");
    expect(within(cmd).getByTestId("setting-version-copy-command")).toBeInTheDocument();
  });

  it("claims 'up to date' only when latest equals installed", async () => {
    fetchUpdateStatusMock.mockResolvedValue(status({ latest_version: "1.58.1", newer_available: false }));
    renderVersionSection();
    expect(await screen.findByTestId("setting-version-uptodate")).toHaveTextContent("up to date");
    expect(within(screen.getByTestId("setting-version-latest")).getByText("v1.58.1").className).not.toContain(
      "text-st-await",
    );
  });

  it("renders — with the reason when the source was unreachable, and — for a never-run check", async () => {
    fetchUpdateStatusMock.mockResolvedValue(
      status({
        latest_version: null,
        newer_available: false,
        reason: "Release source unreachable at last check.",
        last_error: "release source unreachable: http://x: connect refused",
      }),
    );
    renderVersionSection();
    const latest = await screen.findByTestId("setting-version-latest");
    expect(latest).toHaveTextContent("—");
    expect(latest).toHaveTextContent("Release source unreachable at last check.");
    expect(screen.queryByTestId("setting-version-uptodate")).not.toBeInTheDocument();
  });

  it("explains an unknown install method instead of hiding the row, with no Copy button", async () => {
    fetchUpdateStatusMock.mockResolvedValue(
      status({
        install_method: "unknown",
        manual_command: "Build from source, then restart the daemon.",
        supervision: "none",
        checked_at: null,
        latest_version: null,
        newer_available: false,
        reason: "Not checked yet.",
      }),
    );
    renderVersionSection();
    expect(await screen.findByTestId("setting-version-install-method")).toHaveTextContent(
      "Unknown · manual (no service)",
    );
    const cmd = screen.getByTestId("setting-version-manual-command");
    expect(cmd).toHaveTextContent("PDO will not update itself");
    expect(cmd).toHaveTextContent("Build from source, then restart the daemon.");
    expect(within(cmd).queryByTestId("setting-version-copy-command")).not.toBeInTheDocument();
    expect(screen.getByTestId("setting-version-checked-at")).toHaveTextContent("—");
  });

  it("« Check now » posts, spins, refreshes the rows in place and announces on the bus", async () => {
    fetchUpdateStatusMock.mockResolvedValue(status({ latest_version: null, newer_available: false, reason: "Not checked yet.", checked_at: null }));
    let resolve!: (v: UpdateStatus) => void;
    checkForUpdateNowMock.mockReturnValue(new Promise<UpdateStatus>((r) => (resolve = r)));
    const heard = vi.fn();
    window.addEventListener(UPDATE_STATUS_CHANGED, heard);
    renderVersionSection();

    const btn = await screen.findByTestId("setting-version-check-now");
    expect(btn).toHaveTextContent("Check now");
    fireEvent.click(btn);
    expect(checkForUpdateNowMock).toHaveBeenCalledTimes(1);
    expect(btn).toHaveTextContent("Checking…");
    expect(btn).toBeDisabled();

    resolve(status());
    await waitFor(() => expect(btn).toHaveTextContent("Check now"));
    expect(screen.getByTestId("setting-version-latest")).toHaveTextContent("v1.59.0");
    expect(screen.getByTestId("setting-version-checked-at")).toHaveTextContent("GitHub Releases");
    expect(heard).toHaveBeenCalled();
    expect(screen.queryByTestId("setting-version-check-error")).not.toBeInTheDocument();
    window.removeEventListener(UPDATE_STATUS_CHANGED, heard);
  });

  it("a failed « Check now » shows the error in a red row, keeps the values, refreshes the date", async () => {
    const before = status();
    const after = status({ checked_at: new Date().toISOString(), last_error: "release source unreachable: timed out" });
    fetchUpdateStatusMock.mockResolvedValueOnce(before).mockResolvedValueOnce(after);
    checkForUpdateNowMock.mockRejectedValue(new Error("release source unreachable: timed out"));
    renderVersionSection();

    fireEvent.click(await screen.findByTestId("setting-version-check-now"));
    const err = await screen.findByTestId("setting-version-check-error");
    expect(err).toHaveTextContent("Check failed");
    expect(err).toHaveTextContent("release source unreachable: timed out");
    // Last good values kept; the date came from the re-read.
    expect(screen.getByTestId("setting-version-latest")).toHaveTextContent("v1.59.0");
    expect(screen.getByTestId("setting-version-checked-at")).toHaveTextContent("just now");
    expect(fetchUpdateStatusMock).toHaveBeenCalledTimes(2);
  });

  it("the switch writes update_check at the change (not the form's Save), off empties latest and disables Check now", async () => {
    fetchUpdateStatusMock
      .mockResolvedValueOnce(status())
      .mockResolvedValueOnce(status({ check_enabled: false, latest_version: null, newer_available: false, reason: "Update check is off." }));
    updateSettingsMock.mockResolvedValue(settings());
    const heard = vi.fn();
    window.addEventListener(UPDATE_STATUS_CHANGED, heard);
    renderVersionSection();

    const sw = await screen.findByTestId("setting-update-check");
    expect(sw).toHaveAttribute("aria-checked", "true");
    fireEvent.click(sw);
    await waitFor(() => expect(updateSettingsMock).toHaveBeenCalledWith({ update_check: false }));
    await waitFor(() => expect(sw).toHaveAttribute("aria-checked", "false"));
    const latest = screen.getByTestId("setting-version-latest");
    expect(latest).toHaveTextContent("—");
    expect(latest).toHaveTextContent("Update check is off.");
    const btn = screen.getByTestId("setting-version-check-now");
    expect(btn).toBeDisabled();
    expect(btn).toHaveAttribute("title", "Turn the update check on to check now.");
    // The badge follows through the bus, and the form stays clean: nothing to Save.
    expect(heard).toHaveBeenCalled();
    expect(screen.getByText("No unsaved changes")).toBeInTheDocument();
    window.removeEventListener(UPDATE_STATUS_CHANGED, heard);
  });

  it("re-enabling says 'Not checked yet since re-enabling.' until the daemon's check lands", async () => {
    fetchUpdateStatusMock
      .mockResolvedValueOnce(status({ check_enabled: false, latest_version: null, newer_available: false, reason: "Update check is off." }))
      .mockResolvedValueOnce(status({ latest_version: null, newer_available: false, reason: "Not checked yet." }))
      .mockResolvedValue(status());
    updateSettingsMock.mockResolvedValue(settings());
    renderVersionSection();

    const sw = await screen.findByTestId("setting-update-check");
    expect(sw).toHaveAttribute("aria-checked", "false");
    fireEvent.click(sw);
    await waitFor(() => expect(updateSettingsMock).toHaveBeenCalledWith({ update_check: true }));
    expect(await screen.findByText("Not checked yet since re-enabling.")).toBeInTheDocument();
    // The follow-up read picks the landed check up.
    await waitFor(
      () => expect(screen.getByTestId("setting-version-latest")).toHaveTextContent("v1.59.0"),
      { timeout: 3000 },
    );
  });

  it("renders even when GET /settings failed (it does not depend on it)", async () => {
    fetchSettingsMock.mockRejectedValue(new Error("boom"));
    fetchUpdateStatusMock.mockResolvedValue(status());
    renderVersionSection();
    expect(await screen.findByTestId("setting-version-installed")).toHaveTextContent("v1.58.1");
  });
});
