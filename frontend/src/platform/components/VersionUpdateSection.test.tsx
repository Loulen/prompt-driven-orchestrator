import { render, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { UpdateStatus } from "../model/update";

const fetchUpdateStatusMock = vi.fn();
vi.mock("../api/update", () => ({
  fetchUpdateStatus: (...args: unknown[]) => fetchUpdateStatusMock(...args),
  fetchUpdateAttemptLog: vi.fn(),
  checkForUpdateNow: vi.fn(),
  applyUpdate: vi.fn(),
}));

import VersionUpdateSection from "./VersionUpdateSection";
import { SETTINGS_CATEGORIES } from "../../settings/model/settingsCategories";

const SECTION = SETTINGS_CATEGORIES.flatMap((c) => c.sections).find((s) => s.id === "version-update")!;

/** A distribution (#24): its own name, version, source and command, on core 2.0.0. */
function distribution(overrides: Partial<UpdateStatus> = {}): UpdateStatus {
  return {
    channel_name: "acme-pdo",
    installed_version: "0.3.0",
    core_version: "2.0.0",
    latest_version: "0.4.0",
    newer_available: true,
    checked_at: "2026-09-05T08:00:00Z",
    source: "GitHub Releases",
    source_url: "https://api.github.com/repos/acme/acme-pdo/releases/latest",
    source_page_url: "https://github.com/acme/acme-pdo/releases",
    check_enabled: true,
    install_method: "declared",
    manual_command: "brew update && brew upgrade acme/tap/acme-pdo",
    supervision: "systemd",
    reason: null,
    last_error: null,
    active_runs: 0,
    can_apply: true,
    apply_blocked_reason: null,
    last_attempt: null,
    ...overrides,
  };
}

function renderSection() {
  return render(
    <VersionUpdateSection section={SECTION} active onToggleCheck={async () => {}} onRequestUpdate={() => {}} />,
  );
}

describe("Settings › Version & update — a distribution's release channel (#24)", () => {
  beforeEach(() => {
    fetchUpdateStatusMock.mockReset();
  });

  it("shows the distribution's name and version, then the core's", async () => {
    fetchUpdateStatusMock.mockResolvedValue(distribution());
    renderSection();
    const installed = await screen.findByTestId("setting-version-installed");
    expect(installed).toHaveTextContent("acme-pdo 0.3.0(core PDO 2.0.0)");
    // A third party's channel: no mark of the Marque next to its name.
    expect(installed.querySelector('[data-testid="brand-mark"]')).toBeNull();
    expect(screen.getByTestId("setting-version-installed-product")).toHaveTextContent(/^acme-pdo 0\.3\.0$/);
    expect(screen.getByTestId("setting-version-install-method")).toHaveTextContent(
      "Declared by the distribution",
    );
    expect(screen.getByTestId("setting-version-manual-command")).toHaveTextContent(
      "brew upgrade acme/tap/acme-pdo",
    );
    expect(screen.getByTestId("setting-version-update-button")).not.toBeDisabled();
  });

  it("the vanilla binary names the Marque, its mark and its version, no core (CORE-225)", async () => {
    fetchUpdateStatusMock.mockResolvedValue(
      distribution({ channel_name: "PDO", installed_version: "2.0.0", core_version: "2.0.0" }),
    );
    renderSection();
    const installed = await screen.findByTestId("setting-version-installed");
    expect(installed).toHaveTextContent("PDO 2.0.0");
    expect(installed).not.toHaveTextContent("core");
    expect(installed.querySelector('[data-testid="brand-mark"][aria-hidden="true"]')).toHaveAttribute("data-brand", "pdo");
  });

  it("is signed « by Acogni » after its title, whatever the Marque (CORE-225)", async () => {
    document.documentElement.dataset.brand = "acogni-driver";
    try {
      fetchUpdateStatusMock.mockResolvedValue(
        distribution({ channel_name: "Acogni Driver", installed_version: "1.4.0", core_version: "2.76.0" }),
      );
      renderSection();
      const installed = await screen.findByTestId("setting-version-installed");
      expect(installed).toHaveTextContent("Acogni Driver 1.4.0(core PDO 2.76.0)");
      expect(installed.querySelector('[data-testid="brand-mark"]')).toHaveAttribute("data-brand", "acogni-driver");
      const signature = screen.getByTestId("by-acogni");
      expect(signature).toHaveTextContent("by Acogni");
      expect(signature).toHaveClass("set-sec__by");
    } finally {
      delete document.documentElement.dataset.brand;
    }
  });

  it("the source's tooltip is the releases page a human reads, not the API URL (CORE-232)", async () => {
    fetchUpdateStatusMock.mockResolvedValue(distribution());
    renderSection();
    const checked = await screen.findByTestId("setting-version-checked-at");
    const source = within(checked).getByText(/GitHub Releases/);
    expect(source).toHaveAttribute("title", "https://github.com/acme/acme-pdo/releases");
  });

  it("no upgrade command: no Update, no Copy, and the daemon's reason", async () => {
    fetchUpdateStatusMock.mockResolvedValue(
      distribution({
        manual_command: null,
        can_apply: false,
        apply_blocked_reason: "acme-pdo declares no update command: update it the way it was installed, then restart the daemon.",
      }),
    );
    renderSection();
    const btn = await screen.findByTestId("setting-version-update-button");
    expect(btn).toBeDisabled();
    expect(btn).toHaveAttribute("title", expect.stringContaining("declares no update command"));
    expect(
      within(screen.getByTestId("setting-version-manual-command")).getByTitle(/acme-pdo declares no update command/),
    ).toHaveTextContent("none");
    expect(screen.queryByTestId("setting-version-copy-command")).not.toBeInTheDocument();
    expect(screen.getByTestId("setting-version-apply-blocked")).toHaveTextContent("restart the daemon");
  });

  it("no newer version known: Update is not offered, even when the daemon would accept it", async () => {
    fetchUpdateStatusMock.mockResolvedValue(
      distribution({ latest_version: "0.3.0", newer_available: false }),
    );
    renderSection();
    const btn = await screen.findByTestId("setting-version-update-button");
    expect(btn).toBeDisabled();
    expect(btn).toHaveAttribute("title", expect.stringContaining("No newer version"));
  });

  it("no release source: « — » with the reason, and Check now disabled", async () => {
    fetchUpdateStatusMock.mockResolvedValue(
      distribution({
        latest_version: null,
        newer_available: false,
        checked_at: null,
        source: null,
        source_url: null,
        reason: "No release source declared: no version check.",
      }),
    );
    renderSection();
    const latest = await screen.findByTestId("setting-version-latest");
    expect(latest).toHaveTextContent("—");
    expect(latest).toHaveTextContent("No release source declared");
    const check = screen.getByTestId("setting-version-check-now");
    expect(check).toBeDisabled();
    expect(screen.getByTestId("setting-version-update-button")).toBeDisabled();
  });

  it("check off: What's new still opens this build's changelog, and the source is named (CORE-232)", async () => {
    fetchUpdateStatusMock.mockResolvedValue(
      distribution({
        latest_version: null,
        newer_available: false,
        checked_at: null,
        check_enabled: false,
        reason: "Update check is off.",
      }),
    );
    const onOpenChangelog = vi.fn();
    render(
      <VersionUpdateSection
        section={SECTION}
        active
        onToggleCheck={async () => {}}
        onRequestUpdate={() => {}}
        onOpenChangelog={onOpenChangelog}
      />,
    );
    const latest = await screen.findByTestId("setting-version-latest");
    const whatsNew = within(latest).getByTestId("setting-version-whats-new");
    expect(whatsNew).toHaveAttribute("title", "The changelog of this build");
    whatsNew.click();
    expect(onOpenChangelog).toHaveBeenCalled();
    const checked = screen.getByTestId("setting-version-checked-at");
    expect(checked).toHaveTextContent("— · GitHub Releases");
    expect(within(checked).getByText(/GitHub Releases/)).toHaveAttribute("title", "https://github.com/acme/acme-pdo/releases");
  });

  it("a build ahead of the latest release reads « up to date » (CORE-232)", async () => {
    fetchUpdateStatusMock.mockResolvedValue(
      distribution({ installed_version: "0.5.0", latest_version: "0.4.0", newer_available: false }),
    );
    renderSection();
    expect(await screen.findByTestId("setting-version-uptodate")).toHaveTextContent("up to date");
  });
});
