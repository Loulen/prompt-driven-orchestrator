import { afterEach, describe, expect, it } from "vitest";
import { LIT_ATTR, clearLit, markLit } from "./tourLitMark";

function el(): HTMLElement {
  const e = document.createElement("button");
  document.body.appendChild(e);
  return e;
}

afterEach(() => {
  clearLit();
  document.body.innerHTML = "";
});

/**
 * FP of CORE-227 (F3-10b): a control that only shows on hover — a section's
 * « Add output » `+` — was lit at opacity 0, an empty box. The tour marks what
 * it lights, so the stylesheet can show it.
 */
describe("marking what the tour lights", () => {
  it("marks the lit elements, and only them", () => {
    const [a, b] = [el(), el()];
    markLit([a, b]);
    expect(a.hasAttribute(LIT_ATTR)).toBe(true);
    expect(b.hasAttribute(LIT_ATTR)).toBe(true);
  });

  it("unmarks what the next aim no longer lights", () => {
    const [a, b] = [el(), el()];
    markLit([a]);
    markLit([b]);
    expect(a.hasAttribute(LIT_ATTR)).toBe(false);
    expect(b.hasAttribute(LIT_ATTR)).toBe(true);
  });

  it("unmarks everything when the tour stops aiming", () => {
    const a = el();
    markLit([a]);
    clearLit();
    expect(a.hasAttribute(LIT_ATTR)).toBe(false);
  });
});
