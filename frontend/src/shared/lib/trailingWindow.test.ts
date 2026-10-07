import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { trailingWindow } from "./trailingWindow";

describe("trailingWindow", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("runs a burst once, at the end of its window", () => {
    const fn = vi.fn();
    const batched = trailingWindow(fn, 300);
    for (let i = 0; i < 10; i++) batched();
    expect(fn).not.toHaveBeenCalled();
    vi.advanceTimersByTime(299);
    expect(fn).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(fn).toHaveBeenCalledTimes(1);
  });

  it("runs a steady stream once per window, never starving it", () => {
    const fn = vi.fn();
    const batched = trailingWindow(fn, 300);
    for (let t = 0; t < 900; t += 50) {
      batched();
      vi.advanceTimersByTime(50);
    }
    expect(fn).toHaveBeenCalledTimes(3);
  });

  it("drops a pending call when cancelled", () => {
    const fn = vi.fn();
    const batched = trailingWindow(fn, 300);
    batched();
    batched.cancel();
    vi.advanceTimersByTime(1_000);
    expect(fn).not.toHaveBeenCalled();
  });
});
