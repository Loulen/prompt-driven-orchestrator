import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { act, renderHook } from "@testing-library/react";
import { useNodeRun } from "./useNodeRun";
import * as api from "../api";
import type { NodeIO } from "../api";
import type { NodeState } from "../types";

vi.mock("../api", () => ({
  fetchPrompt: vi.fn(),
  fetchNodeIO: vi.fn(),
  markNodeDone: vi.fn(),
  killNode: vi.fn(),
  restartNode: vi.fn(),
  startNode: vi.fn(),
  stopNode: vi.fn(),
  retryNode: vi.fn(),
  retryNodePreview: vi.fn(),
}));

const IO: NodeIO = {
  inputs: [{ port: "brief", repeated: false, files: [] }],
  outputs: [{ port: "plan", repeated: false, files: [] }],
};

function makeNode(overrides?: Partial<NodeState>): NodeState {
  return {
    node_id: "n1",
    status: "running",
    iter: 1,
    started_at: null,
    completed_at: null,
    failure_reason: null,
    iterations: [],
    ...overrides,
  };
}

// `waitFor` needs a real clock, and every cadence assertion below needs a fake
// one — so the promise queue is drained explicitly instead. Two turns: the API
// promise resolves on the first, the state write lands on the second.
async function flush() {
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });
}

async function advance(ms: number) {
  await act(async () => {
    vi.advanceTimersByTime(ms);
    await Promise.resolve();
    await Promise.resolve();
  });
}

beforeEach(() => {
  vi.useFakeTimers();
  vi.mocked(api.fetchPrompt).mockReset().mockResolvedValue("PROMPT");
  vi.mocked(api.fetchNodeIO).mockReset().mockResolvedValue(IO);
  vi.mocked(api.markNodeDone).mockReset().mockResolvedValue({ kind: "completed" });
  vi.mocked(api.retryNodePreview)
    .mockReset()
    .mockResolvedValue({ downstream: [], affected_count: 0, with_artifacts: [] });
  vi.mocked(api.retryNode).mockReset().mockResolvedValue({ ok: true, iter: 2, invalidated: [] });
  vi.mocked(api.stopNode).mockReset().mockResolvedValue(undefined);
  vi.mocked(api.startNode).mockReset().mockResolvedValue({ ok: true, iter: 1 });
  vi.mocked(api.killNode).mockReset().mockResolvedValue(undefined);
  vi.mocked(api.restartNode).mockReset().mockResolvedValue(undefined);
});

afterEach(() => {
  vi.useRealTimers();
});

describe("useNodeRun — prompt fetch (#315 archive guard)", () => {
  it("fetches the prompt once for a live node", async () => {
    const { result } = renderHook(() =>
      useNodeRun("run-1", makeNode({ status: "running" }), 1, {}),
    );
    await flush();
    expect(api.fetchPrompt).toHaveBeenCalledTimes(1);
    expect(api.fetchPrompt).toHaveBeenCalledWith("run-1", "n1", 1);
    expect(result.current.promptText).toBe("PROMPT");
  });

  it("never fetches the prompt for an archived run", async () => {
    const { result } = renderHook(() =>
      useNodeRun("run-1", makeNode({ status: "completed" }), 1, { isArchived: true }),
    );
    await flush();
    expect(api.fetchPrompt).not.toHaveBeenCalled();
    expect(result.current.promptText).toBeNull();
  });

  it("never fetches the prompt for a pending node, archived or not", async () => {
    renderHook(() => useNodeRun("run-1", makeNode({ status: "pending" }), 1, {}));
    await flush();
    expect(api.fetchPrompt).not.toHaveBeenCalled();
  });

  it("does fetch the prompt of a past iteration of a pending node", async () => {
    // A pending node with a stale iter selected: the iteration on screen HAS run,
    // so its rendered prompt exists on disk.
    renderHook(() => useNodeRun("run-1", makeNode({ status: "pending", iter: 3 }), 1, {}));
    await flush();
    expect(api.fetchPrompt).toHaveBeenCalledWith("run-1", "n1", 1);
  });
});

describe("useNodeRun — IO reads", () => {
  it("emits exactly ONE IO fetch and never polls for an archived pending node", async () => {
    const { result } = renderHook(() =>
      useNodeRun("run-1", makeNode({ status: "pending" }), 1, { isArchived: true }),
    );
    await flush();
    expect(api.fetchNodeIO).toHaveBeenCalledTimes(1);
    expect(result.current.inputs).toEqual(IO.inputs);
    expect(result.current.outputs).toEqual(IO.outputs);

    await advance(60_000);
    expect(api.fetchNodeIO).toHaveBeenCalledTimes(1);
  });

  it("reads the IO of an archived non-pending node on the settled cadence (the guard is prompt-only)", async () => {
    renderHook(() =>
      useNodeRun("run-1", makeNode({ status: "completed" }), 1, { isArchived: true }),
    );
    await flush();
    expect(api.fetchNodeIO).toHaveBeenCalledTimes(1);
    await advance(5000);
    expect(api.fetchNodeIO).toHaveBeenCalledTimes(2);
  });

  it("polls IO every 1000ms for a running node", async () => {
    renderHook(() => useNodeRun("run-1", makeNode({ status: "running" }), 1, {}));
    await flush();
    expect(api.fetchNodeIO).toHaveBeenCalledTimes(1);

    await advance(999);
    expect(api.fetchNodeIO).toHaveBeenCalledTimes(1);
    await advance(1);
    expect(api.fetchNodeIO).toHaveBeenCalledTimes(2);
    await advance(2000);
    expect(api.fetchNodeIO).toHaveBeenCalledTimes(4);
  });

  it("polls IO every 1000ms for awaiting_user and stale too", async () => {
    for (const status of ["awaiting_user", "stale"] as const) {
      vi.mocked(api.fetchNodeIO).mockClear();
      const { unmount } = renderHook(() =>
        useNodeRun("run-1", makeNode({ status }), 1, {}),
      );
      await flush();
      await advance(1000);
      expect(api.fetchNodeIO, status).toHaveBeenCalledTimes(2);
      unmount();
    }
  });

  it("polls IO every 5000ms for a terminal node", async () => {
    renderHook(() => useNodeRun("run-1", makeNode({ status: "completed" }), 1, {}));
    await flush();
    expect(api.fetchNodeIO).toHaveBeenCalledTimes(1);

    await advance(4999);
    expect(api.fetchNodeIO).toHaveBeenCalledTimes(1);
    await advance(1);
    expect(api.fetchNodeIO).toHaveBeenCalledTimes(2);
  });

  it("stops polling on unmount", async () => {
    const { unmount } = renderHook(() =>
      useNodeRun("run-1", makeNode({ status: "running" }), 1, {}),
    );
    await flush();
    unmount();
    await advance(5000);
    expect(api.fetchNodeIO).toHaveBeenCalledTimes(1);
  });

  it("reads a past iteration once, without polling, even while the node runs", async () => {
    renderHook(() => useNodeRun("run-1", makeNode({ status: "running", iter: 4 }), 2, {}));
    await flush();
    expect(api.fetchNodeIO).toHaveBeenCalledTimes(1);
    expect(api.fetchNodeIO).toHaveBeenCalledWith("run-1", "n1", 2);

    await advance(10_000);
    expect(api.fetchNodeIO).toHaveBeenCalledTimes(1);
  });

  it("refetches when the selected iteration changes", async () => {
    const { rerender } = renderHook(
      ({ iter }) => useNodeRun("run-1", makeNode({ status: "running", iter: 3 }), iter, {}),
      { initialProps: { iter: 3 } },
    );
    await flush();
    expect(api.fetchNodeIO).toHaveBeenLastCalledWith("run-1", "n1", 3);

    rerender({ iter: 1 });
    await flush();
    expect(api.fetchNodeIO).toHaveBeenLastCalledWith("run-1", "n1", 1);
  });
});

describe("useNodeRun — mark complete (#490)", () => {
  it("occupies the verdict region with `pending` while the call is in flight", async () => {
    // Never settles: the point is that the region has a tenant DURING the call,
    // which is what killed the pre-#490 flicker.
    vi.mocked(api.markNodeDone).mockReturnValue(new Promise<never>(() => {}));
    const { result } = renderHook(() =>
      useNodeRun("run-1", makeNode({ status: "awaiting_user", iter: 3 }), 3, {}),
    );
    await flush();

    await act(async () => {
      result.current.markComplete();
    });
    expect(result.current.markVerdict).toEqual({ iter: 3, kind: "pending" });
  });

  it("stamps the verdict with the SELECTED iter, not node.iter", async () => {
    const { result } = renderHook(() =>
      useNodeRun("run-1", makeNode({ status: "awaiting_user", iter: 4 }), 2, {}),
    );
    await flush();

    await act(async () => {
      await result.current.markComplete();
    });
    expect(api.markNodeDone).toHaveBeenCalledWith("run-1", "n1", 2);
    expect(result.current.markVerdict).toEqual({ iter: 2, kind: "completed" });
  });

  it("carries a refusal verdict through verbatim", async () => {
    vi.mocked(api.markNodeDone).mockResolvedValue({
      kind: "refused",
      slug: "missing_outputs",
      recoverable: true,
      message: "outputs missing",
      missing: ["plan"],
      violations: [],
      body: null,
    });
    const { result } = renderHook(() =>
      useNodeRun("run-1", makeNode({ status: "awaiting_user" }), 1, {}),
    );
    await flush();

    await act(async () => {
      await result.current.markComplete();
    });
    expect(result.current.markVerdict).toMatchObject({
      iter: 1,
      kind: "refused",
      slug: "missing_outputs",
      recoverable: true,
      missing: ["plan"],
    });
  });

  it("turns a transport breakdown into an `error` verdict instead of swallowing it", async () => {
    vi.mocked(api.markNodeDone).mockRejectedValue(new Error("boom"));
    const { result } = renderHook(() =>
      useNodeRun("run-1", makeNode({ status: "awaiting_user" }), 1, {}),
    );
    await flush();

    await act(async () => {
      await result.current.markComplete();
    });
    expect(result.current.markVerdict).toEqual({ iter: 1, kind: "error", message: "boom" });
  });
});

describe("useNodeRun — commands", () => {
  it("stops and starts the node by id", async () => {
    const { result } = renderHook(() =>
      useNodeRun("run-1", makeNode({ status: "running" }), 1, {}),
    );
    await flush();

    await act(async () => {
      await result.current.stop();
      await result.current.start();
    });
    expect(api.stopNode).toHaveBeenCalledWith("run-1", "n1");
    expect(api.startNode).toHaveBeenCalledWith("run-1", "n1");
  });

  it("swallows a command failure (best-effort)", async () => {
    vi.mocked(api.stopNode).mockRejectedValue(new Error("409"));
    const { result } = renderHook(() =>
      useNodeRun("run-1", makeNode({ status: "running" }), 1, {}),
    );
    await flush();

    await act(async () => {
      await expect(result.current.stop()).resolves.toBeUndefined();
    });
  });

  it("sends the stale-banner actions scoped to the SELECTED iter", async () => {
    const { result } = renderHook(() =>
      useNodeRun("run-1", makeNode({ status: "stale", iter: 5 }), 2, {}),
    );
    await flush();

    await act(async () => {
      await result.current.killStale();
      await result.current.restartStale();
    });
    expect(api.killNode).toHaveBeenCalledWith("run-1", "n1", 2);
    expect(api.restartNode).toHaveBeenCalledWith("run-1", "n1", 2);
  });
});

describe("useNodeRun — retry", () => {
  it("retries straight away when nothing downstream is affected, and signals the retry", async () => {
    const onRetryStarted = vi.fn();
    const { result } = renderHook(() =>
      useNodeRun("run-1", makeNode({ status: "running" }), 1, { onRetryStarted }),
    );
    await flush();

    await act(async () => {
      await result.current.retry();
    });
    expect(api.retryNode).toHaveBeenCalledWith("run-1", "n1");
    expect(result.current.retryConfirm).toBeNull();
    expect(onRetryStarted).toHaveBeenCalledTimes(1);
  });

  it("asks for confirmation instead, and does NOT signal a retry that has not happened", async () => {
    vi.mocked(api.retryNodePreview).mockResolvedValue({
      downstream: ["b", "c"],
      affected_count: 2,
      with_artifacts: ["b"],
    });
    const onRetryStarted = vi.fn();
    const { result } = renderHook(() =>
      useNodeRun("run-1", makeNode({ status: "running" }), 1, { onRetryStarted }),
    );
    await flush();

    await act(async () => {
      await result.current.retry();
    });
    expect(result.current.retryConfirm).toEqual({ affectedCount: 2 });
    expect(api.retryNode).not.toHaveBeenCalled();
    expect(onRetryStarted).not.toHaveBeenCalled();

    await act(async () => {
      await result.current.confirmRetry();
    });
    expect(result.current.retryConfirm).toBeNull();
    expect(api.retryNode).toHaveBeenCalledWith("run-1", "n1");
    expect(onRetryStarted).toHaveBeenCalledTimes(1);
  });

  it("drops the confirmation without retrying on cancel", async () => {
    vi.mocked(api.retryNodePreview).mockResolvedValue({
      downstream: ["b"],
      affected_count: 1,
      with_artifacts: [],
    });
    const { result } = renderHook(() =>
      useNodeRun("run-1", makeNode({ status: "running" }), 1, {}),
    );
    await flush();

    await act(async () => {
      await result.current.retry();
    });
    expect(result.current.retryConfirm).toEqual({ affectedCount: 1 });

    await act(async () => {
      result.current.cancelRetry();
    });
    expect(result.current.retryConfirm).toBeNull();
    expect(api.retryNode).not.toHaveBeenCalled();
  });
});
