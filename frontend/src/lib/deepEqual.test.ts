import { describe, it, expect } from "vitest";
import { deepEqual } from "./deepEqual";

describe("deepEqual", () => {
  it("compares primitives", () => {
    expect(deepEqual(1, 1)).toBe(true);
    expect(deepEqual("a", "a")).toBe(true);
    expect(deepEqual(true, false)).toBe(false);
    expect(deepEqual(1, "1")).toBe(false);
    expect(deepEqual(null, null)).toBe(true);
    expect(deepEqual(null, undefined)).toBe(false);
    expect(deepEqual(null, {})).toBe(false);
  });

  it("ignores object key order, recursively", () => {
    expect(deepEqual({ a: 1, b: { c: 2, d: 3 } }, { b: { d: 3, c: 2 }, a: 1 })).toBe(
      true,
    );
  });

  it("distinguishes missing keys from undefined-valued keys count-wise", () => {
    expect(deepEqual({ a: 1 }, { a: 1, b: undefined })).toBe(false);
  });

  it("treats array order as significant", () => {
    expect(deepEqual([1, 2], [2, 1])).toBe(false);
    expect(deepEqual([{ a: 1 }], [{ a: 1 }])).toBe(true);
    expect(deepEqual([1], [1, 2])).toBe(false);
    expect(deepEqual([], [])).toBe(true);
  });

  it("does not confuse arrays with objects", () => {
    expect(deepEqual([], {})).toBe(false);
    expect(deepEqual({ 0: "a", length: 1 }, ["a"])).toBe(false);
  });
});
