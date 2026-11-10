import { describe, expect, it } from "vitest";
import { givenBack, markFocusGivenBack } from "./focusGivenBack";

describe("focusGivenBack", () => {
  it("counts the opener's focus as given back, not another element's", () => {
    const opener = document.createElement("button");
    const other = document.createElement("button");
    document.body.append(opener, other);
    markFocusGivenBack(opener);
    expect(givenBack(opener)).toBe(true);
    expect(givenBack(other)).toBe(false);
  });

  // The full gate: a dialog opened with nothing focused had the body as its
  // opener, and every tooltip of the next test stayed shut for 500 ms.
  it("ignores a body opener, which would hold every element", () => {
    const button = document.createElement("button");
    document.body.append(button);
    markFocusGivenBack(document.body);
    expect(givenBack(button)).toBe(false);
  });
});
