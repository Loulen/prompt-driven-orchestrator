import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import VersionBadge from "./VersionBadge";
import type { UpdateStatus } from "../types";

function status(overrides: Partial<UpdateStatus> = {}): UpdateStatus {
  return {
    installed_version: "1.58.1",
    latest_version: "1.59.0",
    newer_available: true,
    checked_at: "2026-09-05T08:00:00Z",
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

// #697 — the status-bar version is a button; the `→ latest` pill appears ONLY when the
// daemon's cache knows a strictly newer release AND the check is on.
describe("VersionBadge", () => {
  it("shows the installed version as a button that opens Version & update", () => {
    const onClick = vi.fn();
    render(<VersionBadge version="1.58.1" update={status()} onClick={onClick} />);
    const btn = screen.getByTestId("statusbar-version");
    expect(btn.tagName).toBe("BUTTON");
    expect(btn).toHaveTextContent("v1.58.1");
    fireEvent.click(btn);
    expect(onClick).toHaveBeenCalledTimes(1);
  });

  it("wears the `→ latest` pill when a newer release is known", () => {
    render(<VersionBadge version="1.58.1" update={status()} onClick={() => {}} />);
    expect(screen.getByTestId("statusbar-version-badge")).toHaveTextContent("→ 1.59.0");
    expect(screen.getByTestId("statusbar-version")).toHaveAttribute(
      "title",
      expect.stringContaining("v1.59.0 is available"),
    );
  });

  it.each([
    ["up to date", status({ latest_version: "1.58.1", newer_available: false })],
    ["unreachable / unknown", status({ latest_version: null, newer_available: false, reason: "Release source unreachable at last check." })],
    ["check disabled", status({ check_enabled: false, latest_version: null, newer_available: false })],
    ["not loaded yet", null],
  ])("stays a plain version when %s — never an 'up to date' claim in the bar", (_label, update) => {
    render(<VersionBadge version="1.58.1" update={update} onClick={() => {}} />);
    expect(screen.queryByTestId("statusbar-version-badge")).not.toBeInTheDocument();
    expect(screen.getByTestId("statusbar-version")).toHaveTextContent(/^v1\.58\.1$/);
    expect(screen.queryByText(/up to date/i)).not.toBeInTheDocument();
  });
});
