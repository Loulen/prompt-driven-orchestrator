import { describe, expect, it } from "vitest";
import { activeQuery, compilePattern, insertSuggestion, splitRendered } from "./completion";

describe("the query of a source of completion", () => {
  it("opens on the trigger at the start of the text or after a whitespace", () => {
    expect(activeQuery("#", 1, ["#"])).toEqual({ trigger: "#", query: "", start: 0, end: 1 });
    expect(activeQuery("see #bu", 7, ["#"])).toEqual({ trigger: "#", query: "bu", start: 4, end: 7 });
    expect(activeQuery("line\n#x", 7, ["#", "@"])).toEqual({ trigger: "#", query: "x", start: 5, end: 7 });
  });

  it("stays closed in the middle of a word, after a space, or without its trigger", () => {
    expect(activeQuery("a#b", 3, ["#"])).toBeNull();
    expect(activeQuery("#bug ", 5, ["#"])).toBeNull();
    expect(activeQuery("@bob", 4, ["#"])).toBeNull();
    expect(activeQuery("", 0, ["#"])).toBeNull();
  });

  it("follows the caret, not the end of the text", () => {
    expect(activeQuery("#bug fixed", 3, ["#"])).toEqual({ trigger: "#", query: "bu", start: 0, end: 3 });
  });

  it("a pick replaces the trigger and the query, then a space", () => {
    const at = activeQuery("see #bu", 7, ["#"])!;
    expect(insertSuggestion("see #bu", at, "#bug")).toEqual({ text: "see #bug ", caret: 9 });
    const inside = activeQuery("#bu now", 3, ["#"])!;
    expect(insertSuggestion("#bu now", inside, "#bug")).toEqual({ text: "#bug now", caret: 4 });
  });
});

describe("the render of what a source inserted", () => {
  const tag = { source: "test:tags", pattern: compilePattern("#[a-z]+")! };

  it("cuts a text into plain pieces and matches", () => {
    expect(splitRendered("a #bug and #perf.", [tag])).toEqual([
      { kind: "text", text: "a " },
      { kind: "match", source: "test:tags", text: "#bug" },
      { kind: "text", text: " and " },
      { kind: "match", source: "test:tags", text: "#perf" },
      { kind: "text", text: "." },
    ]);
    expect(splitRendered("nothing", [tag])).toEqual([{ kind: "text", text: "nothing" }]);
  });

  it("takes the leftmost match among several renders, never an empty one", () => {
    const at = { source: "x:at", pattern: compilePattern("@\\w+")! };
    const empty = { source: "x:empty", pattern: compilePattern("z*")! };
    expect(splitRendered("@bo #a", [tag, at, empty])).toEqual([
      { kind: "match", source: "x:at", text: "@bo" },
      { kind: "text", text: " " },
      { kind: "match", source: "test:tags", text: "#a" },
    ]);
  });

  it("an invalid pattern compiles to nothing", () => {
    expect(compilePattern("(")).toBeNull();
  });
});
