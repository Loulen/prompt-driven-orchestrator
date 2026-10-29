import { describe, expect, it } from "vitest";
import { readViewed, toggleViewed, viewedKey, writeViewed } from "./viewed";

function memory(): Storage {
  const data = new Map<string, string>();
  return {
    get length() {
      return data.size;
    },
    clear: () => data.clear(),
    getItem: (key) => data.get(key) ?? null,
    key: (index) => [...data.keys()][index] ?? null,
    removeItem: (key) => void data.delete(key),
    setItem: (key, value) => void data.set(key, value),
  };
}

const PAIR = { from: "fork", to: "tip" };

describe("the « Viewed » marks of a Review", () => {
  it("are kept per Run and per pair, in this browser", () => {
    const storage = memory();
    writeViewed("run-1", PAIR, new Set(["src/a.ts"]), storage);
    expect([...readViewed("run-1", PAIR, storage)]).toEqual(["src/a.ts"]);
    expect(readViewed("run-1", { from: "fork", to: "n1:after" }, storage).size).toBe(0);
    expect(readViewed("run-2", PAIR, storage).size).toBe(0);
    expect(storage.getItem(viewedKey("run-1"))).toContain("fork..tip");
  });

  it("toggle a path on and off, and drop a pair left with none", () => {
    const storage = memory();
    const on = toggleViewed(new Set(), "src/a.ts");
    expect([...on]).toEqual(["src/a.ts"]);
    writeViewed("run-1", PAIR, on, storage);
    writeViewed("run-1", PAIR, toggleViewed(on, "src/a.ts"), storage);
    expect(storage.getItem(viewedKey("run-1"))).toBeNull();
  });

  it("read nothing from a value this version did not write", () => {
    const storage = memory();
    storage.setItem(viewedKey("run-1"), "not json");
    expect(readViewed("run-1", PAIR, storage).size).toBe(0);
    storage.setItem(viewedKey("run-1"), JSON.stringify({ "fork..tip": [1, "src/b.ts"] }));
    expect([...readViewed("run-1", PAIR, storage)]).toEqual(["src/b.ts"]);
  });
});
