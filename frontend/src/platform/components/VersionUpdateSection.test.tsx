import { render, screen } from "@testing-library/react";
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

/** A distribution (#24): its own name, version, source and command, on core 2.0.0. */
function distribution(overrides: Partial<UpdateStatus> = {}): UpdateStatus {
  return {
    channel_name: "pdo-dist",
    installed_version: "0.3.0",
    core_version: "2.0.0",
    latest_version: "0.4.0",
    newer_available: true,
    checked_at: "2026-09-05T08:00:00Z",
    source: "GitHub Releases",
    source_url: "https://api.github.com/repos/acme/pdo-dist/releases/latest",
    check_enabled: true,
    install_method: "declared",
    manual_command: "brew update && brew upgrade acme/tap/pdo-dist",
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
    <VersionUpdateSection active onToggleCheck={async () => {}} onRequestUpdate={() => {}} />,
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
    expect(installed).toHaveTextContent("pdo-dist 0.3.0 (core 2.0.0)");
    expect(screen.getByTestId("setting-version-install-method")).toHaveTextContent(
      "Declared by the distribution",
    );
    expect(screen.getByTestId("setting-version-manual-command")).toHaveTextContent(
      "brew upgrade acme/tap/pdo-dist",
    );
    expect(screen.getByTestId("setting-version-update-button")).not.toBeDisabled();
  });

  it("the vanilla binary keeps its bare version", async () => {
    fetchUpdateStatusMock.mockResolvedValue(
      distribution({ channel_name: "PDO", installed_version: "2.0.0", core_version: "2.0.0" }),
    );
    renderSection();
    const installed = await screen.findByTestId("setting-version-installed");
    expect(installed).toHaveTextContent("v2.0.0");
    expect(installed).not.toHaveTextContent("core");
  });

  it("no upgrade command: no Update, no Copy, and the daemon's reason", async () => {
    fetchUpdateStatusMock.mockResolvedValue(
      distribution({
        manual_command: null,
        can_apply: false,
        apply_blocked_reason: "pdo-dist declares no update command: PDO will not guess.",
      }),
    );
    renderSection();
    const btn = await screen.findByTestId("setting-version-update-button");
    expect(btn).toBeDisabled();
    expect(btn).toHaveAttribute("title", expect.stringContaining("declares no update command"));
    expect(screen.getByTestId("setting-version-manual-command")).toHaveTextContent(
      "pdo-dist declares no update command",
    );
    expect(screen.queryByTestId("setting-version-copy-command")).not.toBeInTheDocument();
    expect(screen.getByTestId("setting-version-apply-blocked")).toHaveTextContent("will not guess");
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
        reason: "No release source is declared for this build: the version check does not run.",
      }),
    );
    renderSection();
    const latest = await screen.findByTestId("setting-version-latest");
    expect(latest).toHaveTextContent("—");
    expect(latest).toHaveTextContent("No release source is declared");
    const check = screen.getByTestId("setting-version-check-now");
    expect(check).toBeDisabled();
    expect(check).toHaveAttribute("title", expect.stringContaining("No release source"));
    expect(screen.getByTestId("setting-version-update-button")).toBeDisabled();
  });
});
