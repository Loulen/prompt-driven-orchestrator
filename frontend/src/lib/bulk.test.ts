import { describe, it, expect, vi } from "vitest";
import { runBulk } from "./bulk";

const items = (...ids: string[]) => ids.map((id) => ({ id, label: id.toUpperCase() }));

describe("runBulk", () => {
  it("runs every item and reports all succeeded", async () => {
    const fn = vi.fn().mockResolvedValue(undefined);
    const out = await runBulk(items("a", "b", "c"), fn);
    expect(fn).toHaveBeenCalledTimes(3);
    expect(out.total).toBe(3);
    expect(out.succeeded.map((r) => r.id)).toEqual(["a", "b", "c"]);
    expect(out.failed).toEqual([]);
  });

  it("captures per-item failures without aborting the rest", async () => {
    const fn = vi.fn(async (id: string) => {
      if (id === "b") throw new Error("boom on b");
    });
    const out = await runBulk(items("a", "b", "c"), fn);
    expect(fn).toHaveBeenCalledTimes(3); // c still ran after b failed
    expect(out.succeeded.map((r) => r.id)).toEqual(["a", "c"]);
    expect(out.failed).toHaveLength(1);
    expect(out.failed[0]).toMatchObject({ id: "b", label: "B", ok: false, error: "boom on b" });
  });

  it("never rejects — a rejected fn resolves to a partial outcome", async () => {
    const fn = vi.fn().mockRejectedValue(new Error("all fail"));
    const out = await runBulk(items("a", "b"), fn);
    expect(out.succeeded).toEqual([]);
    expect(out.failed).toHaveLength(2);
  });

  it("reports monotonic progress from 0 to total", async () => {
    const seen: Array<[number, number]> = [];
    await runBulk(items("a", "b"), vi.fn().mockResolvedValue(undefined), (d, t) => seen.push([d, t]));
    expect(seen).toEqual([
      [0, 2],
      [1, 2],
      [2, 2],
    ]);
  });

  it("runs items sequentially, in order", async () => {
    const order: string[] = [];
    const fn = vi.fn(async (id: string) => {
      order.push(`start-${id}`);
      await Promise.resolve();
      order.push(`end-${id}`);
    });
    await runBulk(items("a", "b"), fn);
    // b never starts before a finishes
    expect(order).toEqual(["start-a", "end-a", "start-b", "end-b"]);
  });

  it("handles the empty selection as a trivial success", async () => {
    const fn = vi.fn();
    const out = await runBulk([], fn);
    expect(fn).not.toHaveBeenCalled();
    expect(out).toEqual({ total: 0, succeeded: [], failed: [] });
  });
});
