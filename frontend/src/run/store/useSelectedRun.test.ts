import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { RunState } from "../../shared/generated/RunState";
import { fetchRun } from "../api/runs";
import { SAFETY_NET_MS } from "./useLiveRefresh";
import { useSelectedRun } from "./useSelectedRun";

vi.mock("../api/runs", () => ({ fetchRun: vi.fn() }));

const run = (status: RunState["status"]) => ({ run_id: "r1", status }) as RunState;

/** Let the pending fetch answer. */
async function settle() {
  await act(async () => {
    await Promise.resolve();
  });
}

describe("useSelectedRun", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.mocked(fetchRun).mockReset();
  });
  afterEach(() => vi.useRealTimers());

  // CORE-217: a cost (a node's, a loop's against its cap) climbs while an agent
  // works, with no Run event to say so: a live Run is read again on the net.
  it("reads a live Run again on the safety net, so its costs climb between events", async () => {
    vi.mocked(fetchRun).mockResolvedValue(run("running"));
    const { result } = renderHook(() => useSelectedRun());
    act(() => result.current.select("r1"));
    await settle();
    expect(fetchRun).toHaveBeenCalledTimes(1);

    await act(async () => {
      vi.advanceTimersByTime(SAFETY_NET_MS);
    });
    expect(fetchRun).toHaveBeenCalledTimes(2);
  });

  it("leaves an ended Run alone", async () => {
    vi.mocked(fetchRun).mockResolvedValue(run("completed"));
    const { result } = renderHook(() => useSelectedRun());
    act(() => result.current.select("r1"));
    await settle();

    await act(async () => {
      vi.advanceTimersByTime(SAFETY_NET_MS * 3);
    });
    expect(fetchRun).toHaveBeenCalledTimes(1);
  });
});
