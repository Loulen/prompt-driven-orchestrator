import { describe, expect, it } from "vitest";
import { placeAtCaret } from "./placeAtCaret";

const caret = { top: 100, bottom: 116, left: 40 };
const viewport = { width: 1440, height: 860 };

describe("where a list opened at the caret is drawn", () => {
  it("goes under the caret when it fits there", () => {
    expect(placeAtCaret(caret, { width: 330, height: 200, maxHeight: 280 }, viewport)).toEqual({
      top: 118,
      left: 40,
      maxHeight: 280,
    });
  });

  it("goes above the caret when the room under it is short and above is larger", () => {
    const low = { top: 790, bottom: 806, left: 40 };
    expect(placeAtCaret(low, { width: 330, height: 240, maxHeight: 280 }, viewport)).toEqual({
      bottom: 860 - 790 + 2,
      left: 40,
      maxHeight: 280,
    });
  });

  it("shrinks to the room left on the larger side when it fits on neither", () => {
    const small = { width: 1440, height: 300 };
    const middle = { top: 120, bottom: 136, left: 40 };
    expect(placeAtCaret(middle, { width: 330, height: 240, maxHeight: 280 }, small)).toEqual({
      top: 138,
      left: 40,
      maxHeight: 300 - 138 - 4,
    });
    const lower = { top: 180, bottom: 196, left: 40 };
    expect(placeAtCaret(lower, { width: 330, height: 240, maxHeight: 280 }, small)).toEqual({
      bottom: 300 - 180 + 2,
      left: 40,
      maxHeight: 180 - 2 - 4,
    });
  });

  it("stays inside the viewport's width", () => {
    const right = { top: 100, bottom: 116, left: 1300 };
    expect(placeAtCaret(right, { width: 330, height: 200, maxHeight: 280 }, viewport).left).toBe(1440 - 330 - 4);
    const left = { top: 100, bottom: 116, left: -20 };
    expect(placeAtCaret(left, { width: 330, height: 200, maxHeight: 280 }, viewport).left).toBe(4);
  });
});
