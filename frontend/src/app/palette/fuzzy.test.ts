import { describe, expect, it } from "vitest";
import { fuzzy, isRelevant, substringRanges } from "./fuzzy";

describe("fuzzy", () => {
  it("matches the query's letters in order, spaces ignored", () => {
    expect(fuzzy("adp", "Add a product search")?.hits).toEqual([0, 1, 6]);
    expect(fuzzy("a p s", "Add a product search")).not.toBeNull();
    expect(fuzzy("xyz", "Add a product search")).toBeNull();
    expect(fuzzy("", "anything")).toEqual({ score: 0, hits: [] });
  });

  it("ranks word starts and consecutive letters first", () => {
    const starts = fuzzy("aps", "Add product search")!;
    const scattered = fuzzy("aps", "a-rapids")!;
    expect(starts.score).toBeGreaterThan(scattered.score);
    const run = fuzzy("sea", "search")!;
    const spread = fuzzy("sea", "sxexa")!;
    expect(run.score).toBeGreaterThan(spread.score);
  });

  it("keeps only the matches worth showing", () => {
    expect(isRelevant(fuzzy("prod", "Add a product search"), "prod")).toBe(true);
    expect(isRelevant(fuzzy("adh", "a-xxxxxxdxxxxxxxh"), "adh")).toBe(false);
  });

  it("finds every occurrence of the query in a text", () => {
    expect(substringRanges("Search the search index", "search")).toEqual([
      [0, 6],
      [11, 17],
    ]);
    expect(substringRanges("abc", "  ")).toEqual([]);
  });
});
