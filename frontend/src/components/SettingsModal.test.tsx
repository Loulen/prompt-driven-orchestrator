import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";

const fetchSettingsMock = vi.fn();
const updateSettingsMock = vi.fn();

vi.mock("../api", () => ({
  fetchSettings: (...args: unknown[]) => fetchSettingsMock(...args),
  updateSettings: (...args: unknown[]) => updateSettingsMock(...args),
}));

import SettingsModal from "./SettingsModal";
import { useEditStore } from "../stores/editStore";
import type { InstanceSettings } from "../types";

function sample(overrides: Partial<InstanceSettings> = {}): InstanceSettings {
  return {
    // Cap sourced from env (9) so the shadow-disclosure path is exercised.
    session_cap: { effective: 9, source: "env", stored: null, env: 9, default: 20 },
    reaper_ttl_secs: { effective: 3600, source: "default", stored: null, env: null, default: 3600 },
    guard_timeout_secs: { effective: 60, source: "default", stored: null, env: null, default: 60 },
    updated_at: "2026-07-01T10:00:00.000Z",
    ...overrides,
  };
}

describe("SettingsModal", () => {
  beforeEach(() => {
    fetchSettingsMock.mockReset();
    updateSettingsMock.mockReset();
  });

  it("renders nothing when closed", () => {
    fetchSettingsMock.mockResolvedValue(sample());
    render(<SettingsModal open={false} onClose={() => {}} />);
    expect(screen.queryByTestId("settings-modal")).not.toBeInTheDocument();
  });

  it("loads and seeds the effective values", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    render(<SettingsModal open onClose={() => {}} />);
    const cap = (await screen.findByTestId("setting-session-cap")) as HTMLInputElement;
    expect(cap.value).toBe("9");
    expect((screen.getByTestId("setting-reaper-ttl") as HTMLInputElement).value).toBe("3600");
    expect((screen.getByTestId("setting-guard-timeout") as HTMLInputElement).value).toBe("60");
  });

  it("discloses a shadowed env source for the cap", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    render(<SettingsModal open onClose={() => {}} />);
    const note = await screen.findByTestId("setting-source-session-cap");
    expect(note).toHaveTextContent("PDO_SESSION_CAP=9");
    expect(note).toHaveTextContent(/env/i);
  });

  it("saves only the changed field", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    updateSettingsMock.mockResolvedValue(
      sample({ session_cap: { effective: 4, source: "stored", stored: 4, env: 9, default: 20 } }),
    );
    const onClose = vi.fn();
    const onSaved = vi.fn();
    render(<SettingsModal open onClose={onClose} onSaved={onSaved} />);

    const cap = await screen.findByTestId("setting-session-cap");
    fireEvent.change(cap, { target: { value: "4" } });
    fireEvent.click(screen.getByTestId("settings-save"));

    await waitFor(() => expect(updateSettingsMock).toHaveBeenCalledTimes(1));
    // Only the cap changed; TTL and guard were left at their effective values.
    expect(updateSettingsMock).toHaveBeenCalledWith({ session_cap: 4 });
    await waitFor(() => expect(onSaved).toHaveBeenCalled());
    expect(onClose).toHaveBeenCalled();
  });

  it("rejects invalid input client-side without hitting the API", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    const onClose = vi.fn();
    render(<SettingsModal open onClose={onClose} />);

    const cap = await screen.findByTestId("setting-session-cap");
    fireEvent.change(cap, { target: { value: "0" } });
    fireEvent.click(screen.getByTestId("settings-save"));

    expect(await screen.findByTestId("settings-error")).toBeInTheDocument();
    expect(updateSettingsMock).not.toHaveBeenCalled();
    // Modal stays open on rejection.
    expect(screen.getByTestId("settings-modal")).toBeInTheDocument();
    expect(onClose).not.toHaveBeenCalled();
  });

  it("surfaces a backend rejection in the error banner", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    updateSettingsMock.mockRejectedValue(new Error("session_cap must be >= 1"));
    const onClose = vi.fn();
    render(<SettingsModal open onClose={onClose} />);

    const cap = await screen.findByTestId("setting-session-cap");
    // A value that passes the client check but that the backend rejects.
    fireEvent.change(cap, { target: { value: "4" } });
    fireEvent.click(screen.getByTestId("settings-save"));

    const banner = await screen.findByTestId("settings-error");
    expect(banner).toHaveTextContent("session_cap must be >= 1");
    expect(onClose).not.toHaveBeenCalled();
  });

  it("closes without an API call when nothing changed", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    const onClose = vi.fn();
    render(<SettingsModal open onClose={onClose} />);

    await screen.findByTestId("setting-session-cap");
    fireEvent.click(screen.getByTestId("settings-save"));

    await waitFor(() => expect(onClose).toHaveBeenCalled());
    expect(updateSettingsMock).not.toHaveBeenCalled();
  });

  it("warns when the pending cap enters the tmux-collapse zone", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    render(<SettingsModal open onClose={() => {}} />);
    const cap = await screen.findByTestId("setting-session-cap");
    fireEvent.change(cap, { target: { value: "40" } });
    expect(await screen.findByTestId("settings-cap-advisory")).toBeInTheDocument();
  });
});

describe("SettingsModal — Interface / single-tab toggle (#342)", () => {
  beforeEach(() => {
    fetchSettingsMock.mockReset();
    updateSettingsMock.mockReset();
    localStorage.clear();
    // Reset the shared store so a prior test's toggle doesn't leak in.
    useEditStore.setState({ singleTabMode: false, pendingSingleTab: null, openTabs: [], activeTabId: null });
  });

  it("persists to localStorage at the change, WITHOUT the numeric Save button", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    render(<SettingsModal open onClose={() => {}} />);

    const toggle = await screen.findByTestId("setting-tabs-disabled");
    expect(toggle).toHaveAttribute("aria-checked", "false");

    fireEvent.click(toggle);

    // Written immediately — no `settings-save` click, no PUT.
    expect(localStorage.getItem("pdo.ui.tabsDisabled")).toBe("true");
    expect(useEditStore.getState().singleTabMode).toBe(true);
    expect(updateSettingsMock).not.toHaveBeenCalled();
    expect(toggle).toHaveAttribute("aria-checked", "true");
  });

  it("toggles back off and writes false", async () => {
    fetchSettingsMock.mockResolvedValue(sample());
    render(<SettingsModal open onClose={() => {}} />);
    const toggle = await screen.findByTestId("setting-tabs-disabled");
    fireEvent.click(toggle);
    fireEvent.click(toggle);
    expect(localStorage.getItem("pdo.ui.tabsDisabled")).toBe("false");
    expect(useEditStore.getState().singleTabMode).toBe(false);
  });

  it("stays reachable when GET /settings fails (Trap A — lives in the outer modal)", async () => {
    // Daemon 500: settings never load, the numeric form never mounts…
    fetchSettingsMock.mockRejectedValue(new Error("500"));
    render(<SettingsModal open onClose={() => {}} />);

    // …but the toggle is present and functional.
    const toggle = await screen.findByTestId("setting-tabs-disabled");
    expect(screen.queryByTestId("setting-session-cap")).not.toBeInTheDocument();
    fireEvent.click(toggle);
    expect(localStorage.getItem("pdo.ui.tabsDisabled")).toBe("true");
  });

  it("seeds the toggle from the current store state", async () => {
    useEditStore.setState({ singleTabMode: true });
    fetchSettingsMock.mockResolvedValue(sample());
    render(<SettingsModal open onClose={() => {}} />);
    const toggle = await screen.findByTestId("setting-tabs-disabled");
    expect(toggle).toHaveAttribute("aria-checked", "true");
  });
});
