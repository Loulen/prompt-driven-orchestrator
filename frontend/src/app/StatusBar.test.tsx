import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import StatusBar from "./StatusBar";
import type { UpdateStatus } from "../platform/model/update";

function update(overrides: Partial<UpdateStatus> = {}): UpdateStatus {
  return {
    channel_name: "PDO",
    installed_version: "2.0.0",
    core_version: "2.0.0",
    latest_version: null,
    newer_available: false,
    checked_at: null,
    source: null,
    source_url: null,
    check_enabled: true,
    install_method: "homebrew",
    manual_command: "brew update && brew upgrade Loulen/tap/pdo",
    supervision: "systemd",
    reason: "No release source declared: no version check.",
    last_error: null,
    active_runs: 0,
    can_apply: true,
    apply_blocked_reason: null,
    last_attempt: null,
    ...overrides,
  };
}

function renderBar(u: UpdateStatus | null, sessionsVersion?: string) {
  render(
    <StatusBar
      status="connected"
      sessions={{ live: 0, cap: 4, version: sessionsVersion }}
      update={u}
      onOpenVersion={() => {}}
    />,
  );
  return screen.getByTestId("statusbar-version");
}

// #24 — a distribution names itself in the bar: its version alone would hide the core's.
describe("StatusBar — the version of the release channel", () => {
  it("the vanilla binary shows its bare version, from /sessions", () => {
    expect(renderBar(update(), "2.0.0")).toHaveTextContent(/^v2\.0\.0$/);
  });

  it("a distribution shows its name and version, then the core's", () => {
    const badge = renderBar(
      update({
        channel_name: "acme-pdo",
        installed_version: "0.3.0",
        core_version: "2.0.0",
        install_method: "declared",
        manual_command: "acme-pdo-upgrade",
      }),
      // `/sessions` reports the core's version: it must not be passed off as the distribution's.
      "2.0.0",
    );
    expect(badge).toHaveTextContent("acme-pdo 0.3.0 (core 2.0.0)");
    expect(badge).not.toHaveTextContent("v2.0.0");
  });

  it("a distribution behind its latest release keeps the amber pill", () => {
    const badge = renderBar(
      update({
        channel_name: "acme-pdo",
        installed_version: "0.3.0",
        core_version: "2.0.0",
        latest_version: "0.4.0",
        newer_available: true,
        source: "GitHub Releases",
        source_url: "https://api.github.com/repos/acme/acme-pdo/releases/latest",
        reason: null,
      }),
      "2.0.0",
    );
    expect(badge).toHaveTextContent("acme-pdo 0.3.0 (core 2.0.0)");
    expect(screen.getByTestId("statusbar-version-badge")).toHaveTextContent("→ 0.4.0");
  });
});

// The Workbench's quiet status bar: icons and numbers, their names in the tooltips.
describe("StatusBar — icons and numbers only", () => {
  function renderStatus(status: "connected" | "reconnecting" | "disconnected") {
    render(
      <StatusBar
        status={status}
        sessions={{ live: 3, cap: 20, version: "2.0.0" }}
        update={null}
        onOpenVersion={() => {}}
      />,
    );
  }

  it("draws the daemon's connection as an icon named by its state", () => {
    renderStatus("connected");
    const daemon = screen.getByTestId("daemon-status");
    expect(daemon).toHaveAccessibleName("Daemon: connected");
    expect(daemon).toHaveAttribute("data-st", "connected");
    expect(daemon).toHaveTextContent("");
  });

  it("says a lost connection in its own state", () => {
    renderStatus("disconnected");
    expect(screen.getByTestId("daemon-status")).toHaveAttribute("data-st", "disconnected");
    expect(screen.getByLabelText("Daemon: disconnected")).toBeInTheDocument();
  });

  it("counts the sessions as live/cap, named in its tooltip", async () => {
    renderStatus("connected");
    const counter = screen.getByTestId("session-counter");
    expect(counter).toHaveTextContent(/^3\/20$/);
    fireEvent.pointerEnter(counter);
    fireEvent.pointerMove(counter);
    expect(await screen.findByTestId("tooltip-content")).toHaveTextContent("Sessions");
  });
});
