/**
 * The wheel relay of the Projecteur (#837, point 1). jsdom has no layout, so
 * geometry (`scrollHeight`, `elementsFromPoint`) is stubbed — the logic under
 * test is which element gets the delta, and in what unit.
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import { elementBeneath, relayWheel, scrollableAncestor, wheelDeltaPx } from "./tourScroll";

function scroller(height = 100, content = 400): HTMLDivElement {
  const el = document.createElement("div");
  el.style.overflowY = "auto";
  Object.defineProperty(el, "clientHeight", { value: height, configurable: true });
  Object.defineProperty(el, "scrollHeight", { value: content, configurable: true });
  Object.defineProperty(el, "clientWidth", { value: 300, configurable: true });
  Object.defineProperty(el, "scrollWidth", { value: 300, configurable: true });
  return el;
}

afterEach(() => {
  document.body.innerHTML = "";
  vi.restoreAllMocks();
});

describe("wheelDeltaPx", () => {
  it("passes pixels through", () => {
    expect(wheelDeltaPx({ deltaX: 3, deltaY: 120, deltaMode: 0 }, { width: 10, height: 10 })).toEqual({ dx: 3, dy: 120 });
  });
  it("converts lines and pages", () => {
    expect(wheelDeltaPx({ deltaX: 0, deltaY: 3, deltaMode: 1 }, { width: 10, height: 10 })).toEqual({ dx: 0, dy: 48 });
    expect(wheelDeltaPx({ deltaX: 0, deltaY: 1, deltaMode: 2 }, { width: 10, height: 500 })).toEqual({ dx: 0, dy: 500 });
  });
});

describe("scrollableAncestor", () => {
  it("climbs to the nearest element that overflows", () => {
    const modal = scroller();
    const inner = document.createElement("p");
    modal.appendChild(inner);
    document.body.appendChild(modal);
    expect(scrollableAncestor(inner)).toBe(modal);
  });

  it("falls back to the page when nothing on the way overflows", () => {
    const plain = document.createElement("div");
    document.body.appendChild(plain);
    expect(scrollableAncestor(plain)).toBe(document.scrollingElement ?? document.documentElement);
  });

  it("ignores an overflow:auto box whose content fits", () => {
    const box = scroller(100, 100);
    document.body.appendChild(box);
    expect(scrollableAncestor(box)).toBe(document.scrollingElement ?? document.documentElement);
  });
});

describe("elementBeneath", () => {
  it("skips everything inside the overlay and returns the first app element", () => {
    const overlay = document.createElement("div");
    const blocker = document.createElement("div");
    overlay.appendChild(blocker);
    const modal = document.createElement("div");
    document.body.append(overlay, modal);
    document.elementsFromPoint = vi.fn(() => [blocker, overlay, modal, document.body]);
    expect(elementBeneath(overlay, 10, 10)).toBe(modal);
  });

  it("returns null when only the overlay is there", () => {
    const overlay = document.createElement("div");
    document.body.append(overlay);
    document.elementsFromPoint = vi.fn(() => [overlay]);
    expect(elementBeneath(overlay, 10, 10)).toBeNull();
  });
});

describe("relayWheel", () => {
  it("scrolls the modal under the dim by the wheel's delta", () => {
    const overlay = document.createElement("div");
    const blocker = document.createElement("div");
    overlay.appendChild(blocker);
    const modal = scroller();
    const row = document.createElement("label");
    modal.appendChild(row);
    document.body.append(overlay, modal);
    document.elementsFromPoint = vi.fn(() => [blocker, overlay, row, modal, document.body]);

    const scrolled = relayWheel(overlay, new WheelEvent("wheel", { deltaY: 300, clientX: 5, clientY: 5 }));

    expect(scrolled).toBe(modal);
    expect(modal.scrollTop).toBe(300);
  });
});
