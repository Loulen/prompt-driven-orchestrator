/**
 * The transient-overlay registry (#825) — the thing that stops a modal left open
 * from freezing whoever needs the app next.
 */
import { describe, expect, it, vi } from "vitest";
import { dismissTransientOverlays, registerTransientOverlay } from "./overlays";

describe("dismissing what floats over the app", () => {
  it("closes every registered overlay", () => {
    const first = vi.fn();
    const second = vi.fn();
    const off = [registerTransientOverlay(first), registerTransientOverlay(second)];

    dismissTransientOverlays();

    expect(first).toHaveBeenCalledTimes(1);
    expect(second).toHaveBeenCalledTimes(1);
    for (const unregister of off) unregister();
  });

  it("forgets an overlay that unregistered — an unmounted modal blocks nothing", () => {
    const dismiss = vi.fn();
    registerTransientOverlay(dismiss)();

    dismissTransientOverlays();

    expect(dismiss).not.toHaveBeenCalled();
  });

  /**
   * A dismiss unmounts its own component, whose cleanup deletes it from the very
   * set being walked. Iterating the live set would skip whatever followed it.
   */
  it("survives an overlay that unregisters itself while being dismissed", () => {
    const later = vi.fn();
    let offSelf = () => {};
    offSelf = registerTransientOverlay(() => offSelf());
    const offLater = registerTransientOverlay(later);

    expect(() => dismissTransientOverlays()).not.toThrow();
    expect(later).toHaveBeenCalledTimes(1);
    offLater();
  });

  it("does nothing at all when no overlay is up", () => {
    expect(() => dismissTransientOverlays()).not.toThrow();
  });
});
