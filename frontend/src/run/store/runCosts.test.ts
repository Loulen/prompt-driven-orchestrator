/**
 * The cost the rows of the Runs list show: one read for the asks of a frame,
 * an ended Run read once, a live one again later, nothing shown when no source
 * tells it honestly.
 */
import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { resetRunCosts, useRunCost } from "./runCosts";

const cost = (usd: number, uncosted: string[] = []) => ({
  usd,
  partial: false,
  unpriced_models: [],
  uncosted_harnesses: uncosted,
  by_harness: [],
});

let fetchMock: ReturnType<typeof vi.fn>;

function answer(runs: unknown[]) {
  return new Response(JSON.stringify({ runs }), { status: 200, headers: { "Content-Type": "application/json" } });
}

beforeEach(() => {
  resetRunCosts();
  fetchMock = vi.fn(async (url: string) => {
    const ids = new URL(url, "http://x").searchParams.get("ids")!.split(",");
    return answer(
      ids.map((id) => ({
        run_id: id,
        ...(id === "none" ? {} : { cost: id === "mixed" ? cost(1, ["other"]) : cost(0.4141) }),
      })),
    );
  });
  vi.stubGlobal("fetch", fetchMock);
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe("the cost of a Run's row", () => {
  it("reads the rows of one frame in one request", async () => {
    const a = renderHook(() => useRunCost("a", "completed"));
    const b = renderHook(() => useRunCost("b", "failed"));
    await waitFor(() => expect(a.result.current).toBe(0.4141));
    expect(b.result.current).toBe(0.4141);
    expect(fetchMock).toHaveBeenCalledTimes(1);
    expect(String(fetchMock.mock.calls[0][0])).toContain("/runs/costs?ids=a%2Cb");
  });

  it("reads an ended Run once", async () => {
    const first = renderHook(() => useRunCost("a", "completed"));
    await waitFor(() => expect(first.result.current).toBe(0.4141));
    first.unmount();
    const again = renderHook(() => useRunCost("a", "completed"));
    await act(async () => {});
    expect(again.result.current).toBe(0.4141);
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });

  it("reads a live Run again when it ends", async () => {
    const live = renderHook(({ status }) => useRunCost("a", status), {
      initialProps: { status: "running" as const } as { status: "running" | "completed" },
    });
    await waitFor(() => expect(live.result.current).toBe(0.4141));
    expect(fetchMock).toHaveBeenCalledTimes(1);
    live.rerender({ status: "completed" });
    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(2));
  });

  it("shows nothing when no source tells it, or a harness cannot sum it", async () => {
    const none = renderHook(() => useRunCost("none", "completed"));
    const mixed = renderHook(() => useRunCost("mixed", "completed"));
    await waitFor(() => expect(fetchMock).toHaveBeenCalled());
    await act(async () => {});
    expect(none.result.current).toBeNull();
    expect(mixed.result.current).toBeNull();
  });
});
