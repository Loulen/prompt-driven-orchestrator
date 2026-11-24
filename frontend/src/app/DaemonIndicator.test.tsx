// The daemon's indicator of the status bar: one icon for the connection and
// the Opérations en cours.
import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import DaemonIndicator from "./DaemonIndicator";
import { formatElapsed, ringCount } from "./daemonIndicatorText";
import { beginOperation, resetOperations } from "../shared/http/operations";
import { resetToasts } from "../shared/ui/toast";
import { extensionUiModule } from "../shared/extensions/uiModule";

beforeEach(() => {
  vi.useFakeTimers();
  resetOperations();
  resetToasts();
});

afterEach(() => {
  act(() => resetOperations());
  vi.useRealTimers();
});

function hover(el: HTMLElement) {
  fireEvent.pointerEnter(el);
  fireEvent.pointerMove(el);
  act(() => {
    vi.advanceTimersByTime(400);
  });
}

const indicator = () => screen.getByTestId("daemon-status");

describe("the daemon's indicator", () => {
  it("connected with nothing in progress: a grey check, named in its tooltip", () => {
    render(<DaemonIndicator status="connected" />);
    expect(indicator()).toHaveAttribute("data-ops", "0");
    expect(screen.queryByTestId("daemon-ops-ring")).toBeNull();
    expect(indicator()).toHaveAttribute("aria-live", "polite");
    expect(indicator()).toHaveTextContent("Daemon: connected · nothing in progress");
    hover(indicator());
    expect(screen.getByTestId("tooltip-content")).toHaveTextContent("Daemon: connected · nothing in progress");
  });

  it("shows nothing of an operation shorter than 300 ms", () => {
    render(<DaemonIndicator status="connected" />);
    let end!: () => void;
    act(() => {
      end = beginOperation("Approving node");
    });
    act(() => {
      vi.advanceTimersByTime(250);
    });
    expect(screen.queryByTestId("daemon-ops-ring")).toBeNull();
    act(() => end());
    act(() => {
      vi.advanceTimersByTime(500);
    });
    expect(screen.queryByTestId("daemon-ops-ring")).toBeNull();
  });

  it("one operation past 300 ms: the ruby ring with 1, its label on hover", () => {
    render(<DaemonIndicator status="connected" />);
    act(() => {
      beginOperation("Approving node");
      vi.advanceTimersByTime(300);
    });
    expect(screen.getByTestId("daemon-ops-ring")).toHaveTextContent("1");
    expect(indicator()).toHaveAccessibleName("Approving node");
    hover(indicator());
    expect(screen.getByTestId("tooltip-content")).toHaveTextContent("Approving node");
  });

  it("several operations: their count, and their list with durations on hover", () => {
    render(<DaemonIndicator status="connected" />);
    act(() => {
      beginOperation("Approving node");
      vi.advanceTimersByTime(3000);
      beginOperation("Archiving run");
      vi.advanceTimersByTime(1000);
    });
    expect(screen.getByTestId("daemon-ops-ring")).toHaveTextContent("2");
    expect(indicator()).toHaveAccessibleName("2 operations in progress");
    hover(indicator());
    const rows = screen.getAllByTestId("daemon-ops-row");
    expect(rows).toHaveLength(2);
    expect(rows[0]).toHaveTextContent("Approving node");
    expect(rows[0]).toHaveTextContent(/4 s|5 s/);
    expect(rows[1]).toHaveTextContent("Archiving run");
    expect(screen.getByTestId("daemon-ops-list")).toHaveTextContent("In progress");
  });

  it("the focus shows the list too", () => {
    render(<DaemonIndicator status="connected" />);
    act(() => {
      beginOperation("Approving node");
      beginOperation("Archiving run");
      vi.advanceTimersByTime(300);
    });
    act(() => {
      indicator().focus();
      vi.advanceTimersByTime(400);
    });
    expect(screen.getAllByTestId("daemon-ops-row")).toHaveLength(2);
  });

  it("a lost connection wins: a red cross, the operations still listed on hover", () => {
    render(<DaemonIndicator status="reconnecting" />);
    expect(indicator()).toHaveAttribute("data-st", "reconnecting");
    expect(screen.queryByTestId("daemon-ops-ring")).toBeNull();
    hover(indicator());
    expect(screen.getByTestId("tooltip-content")).toHaveTextContent("Daemon: disconnected, reconnecting…");
    expect(screen.getByTestId("tooltip-content")).toHaveTextContent("Actions are refused until the daemon is back");
    act(() => {
      beginOperation("Archiving run");
      vi.advanceTimersByTime(300);
    });
    expect(screen.queryByTestId("daemon-ops-ring")).toBeNull();
    expect(indicator()).toHaveAccessibleName("Daemon: disconnected, reconnecting…, 1 operation in progress");
  });
});

describe("an extension's operations", () => {
  it("named through the shared module, they show in the same indicator, its icon in its row", () => {
    render(<DaemonIndicator status="connected" />);
    let done!: () => void;
    act(() => {
      void extensionUiModule.trackOperation(
        "Moving note",
        new Promise<void>((resolve) => (done = resolve)),
        { icon: "/ext/test/icon.svg" },
      );
      beginOperation("Archiving run");
      vi.advanceTimersByTime(300);
    });
    expect(screen.getByTestId("daemon-ops-ring")).toHaveTextContent("2");
    hover(indicator());
    const rows = screen.getAllByTestId("daemon-ops-row");
    expect(rows[0]).toHaveTextContent("Moving note");
    expect(rows[0].querySelector(".sh-sb__mini-spin")).toBeNull();
    expect(rows[1].querySelector(".sh-sb__mini-spin")).not.toBeNull();
    act(() => done());
  });
});

describe("the indicator's words", () => {
  it("counts up to 9, then 9+", () => {
    expect(ringCount(3)).toBe("3");
    expect(ringCount(9)).toBe("9");
    expect(ringCount(12)).toBe("9+");
  });

  it("says a duration in tenths under a second, in seconds past it", () => {
    expect(formatElapsed(600)).toBe("0.6 s");
    expect(formatElapsed(1200)).toBe("1 s");
    expect(formatElapsed(4400)).toBe("4 s");
  });
});
