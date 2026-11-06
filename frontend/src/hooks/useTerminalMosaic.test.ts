import { act, renderHook } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import type { TerminalRef } from "../lib/terminalMosaic";

const fetchRunMock = vi.fn();
vi.mock("../api", () => ({
  fetchRun: (...args: unknown[]) => fetchRunMock(...args),
  fetchRuns: vi.fn().mockResolvedValue([]),
}));

import { MOSAIC_STATUS_POLL_MS, useTerminalMosaic } from "./useTerminalMosaic";

function nodeRef(runId: string, nodeId: string): TerminalRef {
  return {
    session: `pdo-${runId}-${nodeId}-iter-1`,
    kind: "node",
    runId,
    runName: runId,
    subject: nodeId,
    paneSource: { runId, nodeId, iter: 1 },
  };
}

function runWith(runId: string, nodeId: string, status: string) {
  return {
    run_id: runId,
    nodes: {
      [nodeId]: {
        node_id: nodeId,
        status,
        iter: 1,
        iterations: [{ iter: 1, status, started_at: null, completed_at: null }],
      },
    },
  };
}

const primary = nodeRef("r1", "design");
const pane = nodeRef("r2", "implement");

function setHidden(hidden: boolean) {
  Object.defineProperty(document, "hidden", { value: hidden, configurable: true });
}

// #1040 — the status poll of the added node panes: `GET /runs/{id}` is a full
// projection, so it must never stack on a slow daemon (#1009).
describe("useTerminalMosaic status poll (#1040)", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    localStorage.clear();
    fetchRunMock.mockReset();
    setHidden(false);
  });
  afterEach(() => {
    vi.useRealTimers();
    setHidden(false);
  });

  function mountWithPane() {
    const hook = renderHook(() => useTerminalMosaic(primary, true));
    act(() => hook.result.current.add(pane));
    return hook;
  }

  it("polls every 10 s after the answer, never twice in flight", async () => {
    let answer: (v: unknown) => void = () => {};
    fetchRunMock.mockImplementation(
      () =>
        new Promise((resolve) => {
          answer = resolve;
        }),
    );
    const hook = mountWithPane();
    await act(() => vi.advanceTimersByTimeAsync(0));
    expect(fetchRunMock).toHaveBeenCalledTimes(1);

    // A slow daemon: no second request while the first is pending.
    await act(() => vi.advanceTimersByTimeAsync(5 * MOSAIC_STATUS_POLL_MS));
    expect(fetchRunMock).toHaveBeenCalledTimes(1);

    await act(async () => answer(runWith("r2", "implement", "running")));
    expect(hook.result.current.statuses[pane.session]).toBe("running");
    await act(() => vi.advanceTimersByTimeAsync(MOSAIC_STATUS_POLL_MS - 1));
    expect(fetchRunMock).toHaveBeenCalledTimes(1);
    await act(() => vi.advanceTimersByTimeAsync(1));
    expect(fetchRunMock).toHaveBeenCalledTimes(2);
    hook.unmount();
  });

  it("does not stack a round when a pane joins while one is pending", async () => {
    let answer: (v: unknown) => void = () => {};
    fetchRunMock.mockImplementation(
      () =>
        new Promise((resolve) => {
          answer = resolve;
        }),
    );
    const hook = mountWithPane();
    await act(() => vi.advanceTimersByTimeAsync(0));
    expect(fetchRunMock).toHaveBeenCalledTimes(1);
    act(() => hook.result.current.add(nodeRef("r3", "review")));
    await act(() => vi.advanceTimersByTimeAsync(0));
    expect(fetchRunMock).toHaveBeenCalledTimes(1);

    fetchRunMock.mockImplementation(async (id: string) =>
      id === "r2" ? runWith("r2", "implement", "running") : runWith("r3", "review", "running"),
    );
    await act(async () => answer(runWith("r2", "implement", "running")));
    await act(() => vi.advanceTimersByTimeAsync(0));
    // The restarted poll ran its round only after the pending one answered.
    expect(fetchRunMock.mock.calls.map(([id]) => id)).toEqual(["r2", "r2", "r3"]);
    hook.unmount();
  });

  it("stops watching a pane once its iteration is frozen", async () => {
    fetchRunMock.mockResolvedValue(runWith("r2", "implement", "completed"));
    const hook = mountWithPane();
    await act(() => vi.advanceTimersByTimeAsync(0));
    expect(hook.result.current.statuses[pane.session]).toBe("completed");

    await act(() => vi.advanceTimersByTimeAsync(10 * MOSAIC_STATUS_POLL_MS));
    expect(fetchRunMock).toHaveBeenCalledTimes(1);

    // Another pane joining does not bring the frozen one back into the poll.
    fetchRunMock.mockResolvedValue(runWith("r3", "review", "running"));
    act(() => hook.result.current.add(nodeRef("r3", "review")));
    await act(() => vi.advanceTimersByTimeAsync(0));
    expect(fetchRunMock.mock.calls.map(([id]) => id)).toEqual(["r2", "r3"]);
    hook.unmount();
  });

  it("skips the rounds due while the tab is hidden", async () => {
    fetchRunMock.mockResolvedValue(runWith("r2", "implement", "running"));
    const hook = mountWithPane();
    await act(() => vi.advanceTimersByTimeAsync(0));
    expect(fetchRunMock).toHaveBeenCalledTimes(1);

    setHidden(true);
    await act(() => vi.advanceTimersByTimeAsync(3 * MOSAIC_STATUS_POLL_MS));
    expect(fetchRunMock).toHaveBeenCalledTimes(1);

    setHidden(false);
    await act(() => vi.advanceTimersByTimeAsync(MOSAIC_STATUS_POLL_MS));
    expect(fetchRunMock).toHaveBeenCalledTimes(2);
    hook.unmount();
  });

  it("does not poll a Manager pane", async () => {
    const hook = renderHook(() => useTerminalMosaic(primary, true));
    act(() =>
      hook.result.current.add({ session: "pdo-mgr-r2", kind: "manager", runId: "r2", runName: "r2", subject: "Manager" }),
    );
    await act(() => vi.advanceTimersByTimeAsync(3 * MOSAIC_STATUS_POLL_MS));
    expect(fetchRunMock).not.toHaveBeenCalled();
    hook.unmount();
  });
});
