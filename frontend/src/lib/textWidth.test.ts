import { afterEach, describe, expect, it, vi } from "vitest";
import { resetTextWidth, uiTextWidth } from "./textWidth";

afterEach(() => {
  vi.unstubAllGlobals();
  document.documentElement.style.removeProperty("--font-ui");
  document.documentElement.style.removeProperty("--fs-sm");
  resetTextWidth();
});

/** A canvas whose text is 5 px per glyph, recording the font it was asked for. */
function stubCanvas() {
  const fonts: string[] = [];
  const ctx = {
    set font(f: string) {
      fonts.push(f);
    },
    measureText: (text: string) => ({ width: text.length * 5 }),
  };
  vi.stubGlobal(
    "OffscreenCanvas",
    class {
      getContext() {
        return ctx;
      }
    },
  );
  resetTextWidth();
  return fonts;
}

describe("uiTextWidth", () => {
  it("has no measure without a canvas, so the caller estimates", () => {
    vi.stubGlobal("OffscreenCanvas", undefined);
    resetTextWidth();
    expect(uiTextWidth("Start")).toBeNull();
  });

  it("measures the text in the UI font at the size token", () => {
    const fonts = stubCanvas();
    document.documentElement.style.setProperty("--font-ui", '"Archivo", sans-serif');
    document.documentElement.style.setProperty("--fs-sm", "12px");
    expect(uiTextWidth("Start")).toBe(25);
    expect(fonts.at(-1)).toBe('450 12px "Archivo", sans-serif');
  });
});
