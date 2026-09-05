import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import UpdateConfirmModal from "./UpdateConfirmModal";
import type { UpdateStatus } from "../types";

// #699 — the confirm before an in-app update: the command, the restart, the active
// Runs count (a warning, never a block), tmux survival.

function status(overrides: Partial<UpdateStatus> = {}): UpdateStatus {
  return {
    installed_version: "1.60.0",
    latest_version: "1.61.0",
    newer_available: true,
    checked_at: null,
    source: "GitHub Releases",
    source_url: "",
    check_enabled: true,
    install_method: "homebrew",
    manual_command: "brew update && brew upgrade Loulen/tap/pdo",
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

describe("UpdateConfirmModal (#699)", () => {
  it("names the target version, the exact command and the service restart", () => {
    render(<UpdateConfirmModal update={status()} onConfirm={() => {}} onCancel={() => {}} />);
    expect(screen.getByText("Update PDO to v1.61.0?")).toBeInTheDocument();
    expect(screen.getByTestId("update-confirm-command")).toHaveTextContent(
      "brew update && brew upgrade Loulen/tap/pdo",
    );
    expect(screen.getByText(/stable binary path, then the systemd service restarts/)).toBeInTheDocument();
    expect(screen.getByTestId("update-confirm-runs")).toHaveTextContent("No Run is active");
  });

  it("counts the active Runs and promises tmux survival — the button stays enabled", () => {
    const onConfirm = vi.fn();
    render(<UpdateConfirmModal update={status({ active_runs: 3 })} onConfirm={onConfirm} onCancel={() => {}} />);
    const runs = screen.getByTestId("update-confirm-runs");
    expect(runs).toHaveTextContent("3 Runs are active");
    expect(runs).toHaveTextContent("tmux sessions survive");
    expect(runs.className).toContain("text-st-await");
    const ok = screen.getByTestId("update-confirm-ok");
    expect(ok).not.toBeDisabled();
    fireEvent.click(ok);
    expect(onConfirm).toHaveBeenCalledTimes(1);
  });

  it("singular for one Run; manual daemon wording when unsupervised", () => {
    render(
      <UpdateConfirmModal
        update={status({ active_runs: 1, supervision: "none" })}
        onConfirm={() => {}}
        onCancel={() => {}}
      />,
    );
    expect(screen.getByTestId("update-confirm-runs")).toHaveTextContent("1 Run is active");
    expect(screen.getByText(/stopped and relaunched with its current arguments/)).toBeInTheDocument();
  });

  it("Cancel and Escape both cancel; busy disables the confirm", () => {
    const onCancel = vi.fn();
    render(<UpdateConfirmModal update={status()} busy onConfirm={() => {}} onCancel={onCancel} />);
    expect(screen.getByTestId("update-confirm-ok")).toBeDisabled();
    expect(screen.getByTestId("update-confirm-ok")).toHaveTextContent("Starting…");
    fireEvent.click(screen.getByTestId("update-confirm-cancel"));
    fireEvent.keyDown(document, { key: "Escape" });
    expect(onCancel).toHaveBeenCalledTimes(2);
  });
});
