import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { pollAfterResponse } from "./pollAfterResponse";

function deferred() {
  let resolve: (v?: boolean | void) => void = () => {};
  let reject: (e: unknown) => void = () => {};
  const promise = new Promise<boolean | void>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

describe("pollAfterResponse (#1040)", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("runs at once, then only once the previous round settled — never two in flight", async () => {
    const rounds: ReturnType<typeof deferred>[] = [];
    const round = vi.fn(() => {
      const d = deferred();
      rounds.push(d);
      return d.promise;
    });
    const stop = pollAfterResponse(round, 10_000, () => false);
    expect(round).toHaveBeenCalledTimes(1);

    // The daemon is slow: a minute passes, still one round.
    await vi.advanceTimersByTimeAsync(60_000);
    expect(round).toHaveBeenCalledTimes(1);

    await vi.waitFor(() => rounds[0].resolve());
    await vi.advanceTimersByTimeAsync(9_999);
    expect(round).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(1);
    expect(round).toHaveBeenCalledTimes(2);
    stop();
  });

  it("goes on after a failed round", async () => {
    const round = vi.fn().mockRejectedValueOnce(new Error("down")).mockResolvedValue(undefined);
    const stop = pollAfterResponse(round, 10_000, () => false);
    await vi.advanceTimersByTimeAsync(10_000);
    expect(round).toHaveBeenCalledTimes(2);
    stop();
  });

  it("skips the rounds due while the tab is hidden", async () => {
    let hidden = true;
    const round = vi.fn().mockResolvedValue(undefined);
    const stop = pollAfterResponse(round, 10_000, () => hidden);
    await vi.advanceTimersByTimeAsync(30_000);
    expect(round).not.toHaveBeenCalled();
    hidden = false;
    await vi.advanceTimersByTimeAsync(10_000);
    expect(round).toHaveBeenCalledTimes(1);
    stop();
  });

  it("ends when a round says so, or when stopped", async () => {
    const done = vi.fn().mockResolvedValue(false);
    pollAfterResponse(done, 10_000, () => false);
    await vi.advanceTimersByTimeAsync(50_000);
    expect(done).toHaveBeenCalledTimes(1);

    const round = vi.fn().mockResolvedValue(undefined);
    const stop = pollAfterResponse(round, 10_000, () => false);
    await vi.advanceTimersByTimeAsync(0);
    stop();
    await vi.advanceTimersByTimeAsync(50_000);
    expect(round).toHaveBeenCalledTimes(1);
  });
});
