import { renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { WsMessage } from "../../run/model/liveStream";
import type { RunState } from "../../shared/generated/RunState";

let listener: ((msg: WsMessage) => void) | null = null;
vi.mock("../../run/store/useDaemonSocket", () => ({
  useDaemonSocket: () => ({
    status: "connected",
    subscribe: (l: (msg: WsMessage) => void) => {
      listener = l;
      return () => {
        listener = null;
      };
    },
  }),
}));
const fetchRun = vi.fn(async (runId: string) => ({ run_id: runId, nodes: [], review_comments: [] }) as unknown as RunState);
vi.mock("../../run/api/runs", () => ({ fetchRun: (runId: string) => fetchRun(runId) }));

const { useReviewLive } = await import("./useReviewLive");

function mount() {
  const setRun = vi.fn();
  renderHook(() =>
    useReviewLive({
      runId: "r1",
      runRef: { current: null },
      setRun,
      sigAtLoad: { current: null },
      pairRef: { current: { from: "fork", to: "tip" } },
      onTipMoved: () => {},
    }),
  );
  return { setRun };
}

describe("useReviewLive", () => {
  beforeEach(() => {
    fetchRun.mockClear();
  });

  it("reads the Run again on one of its events, not on another Run's", async () => {
    const { setRun } = mount();
    listener?.({ type: "event", event: { run_id: "other" } } as WsMessage);
    expect(fetchRun).not.toHaveBeenCalled();
    listener?.({ type: "event", event: { run_id: "r1", kind: "node_started" } } as WsMessage);
    expect(fetchRun).toHaveBeenCalledWith("r1");
    await vi.waitFor(() => expect(setRun).toHaveBeenCalledTimes(1));
  });

  it("reads the Run again on a resync", async () => {
    const { setRun } = mount();
    listener?.({ type: "resync" });
    expect(fetchRun).toHaveBeenCalledWith("r1");
    await vi.waitFor(() => expect(setRun).toHaveBeenCalledTimes(1));
  });
});
