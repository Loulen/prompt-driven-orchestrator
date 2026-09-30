import { describe, expect, it } from "vitest";
import { splitDeepLinks } from "./remarkExtensionLinks";

describe("deep links written as plain text", () => {
  it("become links, trailing punctuation left as text", () => {
    expect(splitDeepLinks("see #/ext/audit/journal. Then")).toEqual([
      { type: "text", value: "see " },
      { type: "link", url: "#/ext/audit/journal", children: [{ type: "text", value: "#/ext/audit/journal" }] },
      { type: "text", value: ". Then" },
    ]);
  });

  it("leave text without one untouched", () => {
    expect(splitDeepLinks("#/extra and #/ext/")).toEqual([{ type: "text", value: "#/extra and #/ext/" }]);
  });
});
