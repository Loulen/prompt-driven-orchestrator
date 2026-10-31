import { describe, expect, it } from "vitest";
import { frontmatterEntries, frontmatterValue, stripFrontmatter } from "./frontmatter";

describe("stripFrontmatter", () => {
  it("drops the leading fence and keeps the body", () => {
    expect(stripFrontmatter("---\nverdict: PASS\n---\n\n# Review\n")).toBe("# Review\n");
    expect(stripFrontmatter("# No fence\n")).toBe("# No fence\n");
  });
});

describe("frontmatterEntries", () => {
  it("lists the fields in the order the file writes them", () => {
    const content = "---\nverdict: PASS\nscore: 3\nreviewers:\n  - alice\n  - bob\nauthor: carol\n---\nBody";
    // The daemon answers a map: its key order is not the file's.
    const fields = { author: "carol", reviewers: ["alice", "bob"], score: 3, verdict: "PASS" };
    expect(frontmatterEntries(fields, content).map(([k]) => k)).toEqual([
      "verdict",
      "score",
      "reviewers",
      "author",
    ]);
  });

  it("keeps the daemon's fields the fence does not show, after the others", () => {
    expect(frontmatterEntries({ b: 1, a: 2 }, "---\na: 2\n---\n").map(([k]) => k)).toEqual(["a", "b"]);
    expect(frontmatterEntries({ b: 1, a: 2 }, null).map(([k]) => k)).toEqual(["b", "a"]);
  });

  it("reads only the top-level keys of the fence", () => {
    const content = "---\nmeta:\n  zeta: 1\n  alpha: 2\nname: x\n---\n";
    expect(frontmatterEntries({ name: "x", meta: { zeta: 1, alpha: 2 } }, content).map(([k]) => k)).toEqual([
      "meta",
      "name",
    ]);
  });
});

describe("frontmatterValue", () => {
  it("reads a list as its items, not as JSON", () => {
    expect(frontmatterValue(["alice", "bob", 3])).toBe("alice, bob, 3");
  });

  it("reads scalars as they are written", () => {
    expect(frontmatterValue("PASS")).toBe("PASS");
    expect(frontmatterValue(4.5)).toBe("4.5");
    expect(frontmatterValue(true)).toBe("true");
    expect(frontmatterValue(null)).toBe("");
  });

  it("reads a map as its pairs", () => {
    expect(frontmatterValue({ zeta: 1, tags: ["a", "b"] })).toBe("zeta: 1, tags: [a, b]");
  });
});
