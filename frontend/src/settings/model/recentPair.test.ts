import { describe, expect, it } from "vitest";
import { recentPair } from "./recentPair";

type Item = { id: string; at?: string; mark?: boolean };
const pair = (items: Item[]) => recentPair(items, (i) => i.at, (i) => Boolean(i.mark)).map((i) => i.id);

describe("recentPair", () => {
  it("is empty without an item, and the item alone with one", () => {
    expect(pair([])).toEqual([]);
    expect(pair([{ id: "a" }])).toEqual(["a"]);
  });

  it("takes the two latest, an undated one after, in the list's order", () => {
    expect(pair([{ id: "none" }, { id: "old", at: "1" }, { id: "new", at: "2" }])).toEqual(["new", "old"]);
    expect(pair([{ id: "x" }, { id: "y" }, { id: "z" }])).toEqual(["x", "y"]);
  });

  it("takes the latest wanted one second when the latest is not", () => {
    expect(pair([{ id: "new", at: "3" }, { id: "mid", at: "2" }, { id: "marked", at: "1", mark: true }])).toEqual([
      "new",
      "marked",
    ]);
    expect(pair([{ id: "new", at: "3", mark: true }, { id: "mid", at: "2" }, { id: "m", at: "1", mark: true }])).toEqual([
      "new",
      "mid",
    ]);
  });
});
