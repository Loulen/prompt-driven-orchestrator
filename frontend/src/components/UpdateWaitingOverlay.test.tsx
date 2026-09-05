import { render, screen, fireEvent, act } from "@testing-library/react";
import { describe, it, expect, vi, afterEach } from "vitest";
import UpdateWaitingOverlay from "./UpdateWaitingOverlay";
import { IDLE, UPDATE_SLOW_AFTER_MS, type UpdateFlowState } from "../lib/updateFlow";

// #699 — the waiting screen: phase messages, the slow hint, the failure card.

function flow(overrides: Partial<UpdateFlowState>): UpdateFlowState {
  return { ...IDLE, attemptId: "a1", fromVersion: "1.60.0", startedAt: Date.now(), ...overrides };
}

afterEach(() => {
  vi.useRealTimers();
});

describe("UpdateWaitingOverlay (#699)", () => {
  it("renders nothing when idle", () => {
    render(<UpdateWaitingOverlay flow={IDLE} onDismiss={() => {}} onOpenVersionSettings={() => {}} />);
    expect(screen.queryByTestId("update-waiting")).not.toBeInTheDocument();
  });

  it.each([
    ["applying", /Running the update command/],
    ["restarting", /restarting — reconnecting/],
    ["verifying", /checking the version/],
    ["reload", /reloading the page/],
  ] as const)("phase %s shows its message", (phase, re) => {
    render(<UpdateWaitingOverlay flow={flow({ phase })} onDismiss={() => {}} onOpenVersionSettings={() => {}} />);
    expect(screen.getByTestId("update-waiting")).toHaveAttribute("data-phase", phase);
    expect(screen.getByTestId("update-waiting-message")).toHaveTextContent(re);
    expect(screen.getByText(/Updating PDO from v1.60.0/)).toBeInTheDocument();
    expect(screen.queryByTestId("update-waiting-slow")).not.toBeInTheDocument();
  });

  it("says so after the slow threshold and offers to keep the old version", () => {
    vi.useFakeTimers();
    const onDismiss = vi.fn();
    render(
      <UpdateWaitingOverlay
        flow={flow({ phase: "restarting", startedAt: Date.now() - UPDATE_SLOW_AFTER_MS + 500 })}
        onDismiss={onDismiss}
        onOpenVersionSettings={() => {}}
      />,
    );
    expect(screen.queryByTestId("update-waiting-slow")).not.toBeInTheDocument();
    act(() => {
      vi.advanceTimersByTime(2000);
    });
    expect(screen.getByTestId("update-waiting-slow")).toHaveTextContent(/taking longer than expected/);
    fireEvent.click(screen.getByTestId("update-waiting-dismiss"));
    expect(onDismiss).toHaveBeenCalled();
  });

  it("a failed attempt is a dismissable card with the reason and a link to the log", () => {
    const onOpen = vi.fn();
    const onDismiss = vi.fn();
    render(
      <UpdateWaitingOverlay
        flow={flow({
          phase: "failed",
          error: "The update command exited with code 3.",
          attempt: {
            attempt_id: "a1",
            status: "failed",
            started_at: "2026-09-05T12:00:00Z",
            finished_at: "2026-09-05T12:00:05Z",
            exit_code: 3,
            method: "homebrew",
            command: "brew update && brew upgrade Loulen/tap/pdo",
            supervision: "systemd",
            log_path: "/home/u/.pdo/update/a1.log",
            from_version: "1.60.0",
          },
        })}
        onDismiss={onDismiss}
        onOpenVersionSettings={onOpen}
      />,
    );
    expect(screen.getByText("Update failed")).toBeInTheDocument();
    expect(screen.getByTestId("update-waiting-error")).toHaveTextContent("exited with code 3");
    expect(screen.getByText(/attempt a1 · brew update/)).toBeInTheDocument();
    fireEvent.click(screen.getByTestId("update-waiting-open-settings"));
    expect(onOpen).toHaveBeenCalled();
    fireEvent.click(screen.getByTestId("update-waiting-dismiss"));
    expect(onDismiss).toHaveBeenCalled();
  });

  it("the same version after the restart is reported as not taken effect", () => {
    render(
      <UpdateWaitingOverlay
        flow={flow({ phase: "same-version", error: "The daemon came back on v1.60.0 — the binary did not change." })}
        onDismiss={() => {}}
        onOpenVersionSettings={() => {}}
      />,
    );
    expect(screen.getByText("Update did not take effect")).toBeInTheDocument();
    expect(screen.getByTestId("update-waiting-error")).toHaveTextContent("did not change");
  });
});
