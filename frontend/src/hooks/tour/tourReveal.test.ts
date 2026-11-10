/**
 * Whether a tour target is cut off, on both axes and by a clipping pane — the
 * test the pump scrolls on (FP of CORE-227: the « Add output » `+` lit half off
 * the window's right edge).
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import { cutOff, revealTarget } from "./tourReveal";

type Rect = { top: number; left: number; width: number; height: number };

function boxed(el: Element, r: Rect) {
  vi.spyOn(el, "getBoundingClientRect").mockReturnValue({
    ...r,
    bottom: r.top + r.height,
    right: r.left + r.width,
    x: r.left,
    y: r.top,
    toJSON: () => r,
  } as DOMRect);
}

afterEach(() => {
  vi.restoreAllMocks();
  document.body.innerHTML = "";
});

describe("a target cut off", () => {
  it("is not, well inside the window", () => {
    const el = document.createElement("button");
    document.body.append(el);
    boxed(el, { top: 100, left: 100, width: 20, height: 20 });
    expect(cutOff(el)).toBe(false);
  });

  it("is, past the window's right edge — not only past its bottom", () => {
    const el = document.createElement("button");
    document.body.append(el);
    boxed(el, { top: 100, left: window.innerWidth - 8, width: 20, height: 20 });
    expect(cutOff(el)).toBe(true);
  });

  it("is, past the box of a pane that clips it, though the window has room", () => {
    const pane = document.createElement("div");
    pane.style.overflowY = "auto";
    const el = document.createElement("button");
    pane.append(el);
    document.body.append(pane);
    boxed(pane, { top: 0, left: 0, width: 300, height: 200 });
    boxed(el, { top: 190, left: 10, width: 20, height: 20 });
    expect(cutOff(el)).toBe(true);
  });

  it("is not, past a box that lets it overflow", () => {
    const wrapper = document.createElement("div");
    const el = document.createElement("button");
    wrapper.append(el);
    document.body.append(wrapper);
    boxed(wrapper, { top: 0, left: 0, width: 10, height: 10 });
    boxed(el, { top: 100, left: 100, width: 20, height: 20 });
    expect(cutOff(el)).toBe(false);
  });
});

describe("revealing it", () => {
  it("scrolls on both axes when it is cut off, and not when it is whole", () => {
    const scrollIntoView = vi.fn();
    const original = Element.prototype.scrollIntoView;
    Element.prototype.scrollIntoView = scrollIntoView;
    try {
      const el = document.createElement("button");
      document.body.append(el);
      boxed(el, { top: 100, left: 100, width: 20, height: 20 });
      revealTarget(el);
      expect(scrollIntoView).not.toHaveBeenCalled();

      boxed(el, { top: 100, left: window.innerWidth - 8, width: 20, height: 20 });
      revealTarget(el);
      expect(scrollIntoView).toHaveBeenCalledWith({ block: "center", inline: "nearest", behavior: "smooth" });
    } finally {
      if (original) Element.prototype.scrollIntoView = original;
      else Reflect.deleteProperty(Element.prototype, "scrollIntoView");
    }
  });
});
