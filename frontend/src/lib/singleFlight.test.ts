import { afterEach, describe, expect, it, vi } from "vitest";
import { pollAfterResponse, singleFlight } from "./singleFlight";

/** A call whose completion the test controls. */
function gate() {
  let open!: () => void;
  const opened = new Promise<void>((resolve) => {
    open = resolve;
  });
  return { opened, open };
}

describe("singleFlight", () => {
  it("runs one call at a time and folds a burst into one trailing call", async () => {
    const gates = [gate(), gate()];
    let calls = 0;
    let inFlight = 0;
    let maxInFlight = 0;
    const refresh = singleFlight(async () => {
      const g = gates[calls++];
      inFlight++;
      maxInFlight = Math.max(maxInFlight, inFlight);
      await g.opened;
      inFlight--;
    });

    const first = refresh();
    // Ten triggers while the first request is pending.
    for (let i = 0; i < 10; i++) void refresh();
    expect(calls).toBe(1);

    gates[0].open();
    await vi.waitFor(() => expect(calls).toBe(2));
    gates[1].open();
    await first;

    expect(calls).toBe(2);
    expect(maxInFlight).toBe(1);
  });

  it("starts fresh once idle, and survives a failing call", async () => {
    let calls = 0;
    const refresh = singleFlight(async () => {
      calls++;
      if (calls === 1) throw new Error("daemon down");
    });

    await refresh();
    await refresh();

    expect(calls).toBe(2);
  });

  it("does not run a trailing call when nothing arrived meanwhile", async () => {
    let calls = 0;
    const refresh = singleFlight(async () => {
      calls++;
    });

    await refresh();

    expect(calls).toBe(1);
  });
});

describe("pollAfterResponse", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("waits for the answer before scheduling the next call", async () => {
    vi.useFakeTimers();
    const gates = [gate(), gate()];
    let calls = 0;
    const stop = pollAfterResponse(async () => {
      await gates[calls++]?.opened;
    }, 3000);

    expect(calls).toBe(1);
    // The daemon is slow: 10 s pass without an answer — no second call.
    await vi.advanceTimersByTimeAsync(10_000);
    expect(calls).toBe(1);

    gates[0].open();
    await vi.advanceTimersByTimeAsync(2_999);
    expect(calls).toBe(1);
    await vi.advanceTimersByTimeAsync(1);
    expect(calls).toBe(2);

    stop();
    gates[1].open();
    await vi.advanceTimersByTimeAsync(10_000);
    expect(calls).toBe(2);
  });

  it("keeps polling after a failed call", async () => {
    vi.useFakeTimers();
    let calls = 0;
    const stop = pollAfterResponse(async () => {
      calls++;
      throw new Error("network");
    }, 1000);

    await vi.advanceTimersByTimeAsync(2_500);

    expect(calls).toBe(3);
    stop();
  });
});
