/**
 * The canvas-reveal seam (#825, FP iteration 2) — how a tour asks for a node it
 * cannot scroll to, without knowing anything about xyflow.
 */
import { describe, expect, it, vi } from "vitest";
import { canvasNodeIdOf, registerCanvasReveal, revealCanvasNode } from "./canvasReveal";

describe("asking the canvas to show a node", () => {
  it("reaches the mounted canvas, with the node that was asked for", () => {
    const reveal = vi.fn();
    const off = registerCanvasReveal(reveal);

    revealCanvasNode("n1");

    expect(reveal).toHaveBeenCalledWith("n1");
    off();
  });

  /** No canvas on screen is not an error: the caller is describing a wish, and a
   *  tour that threw because the pipeline tab was closed would be worse than a
   *  tour that lit nothing. */
  it("is a no-op when no canvas is mounted", () => {
    expect(() => revealCanvasNode("n1")).not.toThrow();
  });

  /** One canvas at a time — the active tab's. An unregister that fired after a
   *  second canvas had registered would silence the one actually on screen. */
  it("keeps the canvas that registered last, and ignores a stale unregister", () => {
    const first = vi.fn();
    const second = vi.fn();
    const offFirst = registerCanvasReveal(first);
    const offSecond = registerCanvasReveal(second);

    offFirst();
    revealCanvasNode("n1");

    expect(first).not.toHaveBeenCalled();
    expect(second).toHaveBeenCalledWith("n1");
    offSecond();
  });
});

describe("recognising a canvas card", () => {
  /** xyflow stamps `data-id` on `.react-flow__node`, which is what every canvas
   *  target selector in `lib/tours/` is built on. */
  it("reads the node id off the card, from the card or from inside it", () => {
    document.body.innerHTML = `
      <div class="react-flow__node" data-id="n1"><button id="inner">x</button></div>
      <div id="elsewhere"></div>`;

    expect(canvasNodeIdOf(document.querySelector(".react-flow__node")!)).toBe("n1");
    expect(canvasNodeIdOf(document.getElementById("inner")!)).toBe("n1");
    expect(canvasNodeIdOf(document.getElementById("elsewhere")!)).toBeNull();
  });
});
