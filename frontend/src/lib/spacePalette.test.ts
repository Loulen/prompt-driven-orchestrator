import { describe, it, expect } from "vitest";
import { firstFreeSpaceColor, SPACE_COLORS, spaceColorVar } from "./spacePalette";

describe("Espace palette (#1041)", () => {
  it("has eight keys, in the daemon's order (`space_store::PALETTE`)", () => {
    expect([...SPACE_COLORS]).toEqual([
      "blue",
      "orange",
      "green",
      "pink",
      "sky",
      "vermillion",
      "violet",
      "yellow",
    ]);
  });

  it("gives a new Espace the first free colour", () => {
    expect(firstFreeSpaceColor([])).toBe("blue");
    expect(firstFreeSpaceColor(["blue"])).toBe("orange");
    // A gap left by a deleted or recoloured Espace is filled first.
    expect(firstFreeSpaceColor(["blue", "green"])).toBe("orange");
  });

  it("once all eight are taken, picks the first of the least used", () => {
    expect(firstFreeSpaceColor([...SPACE_COLORS])).toBe("blue");
    expect(firstFreeSpaceColor([...SPACE_COLORS, "blue"])).toBe("orange");
  });

  it("paints through the theme token, and a neutral grey for an unknown key", () => {
    expect(spaceColorVar("violet")).toBe("var(--color-space-violet)");
    expect(spaceColorVar("#ff0000")).toBe("var(--color-fg-4)");
  });
});
