/**
 * The transient-overlay registry (#825) — the thing that stops a modal left open
 * from freezing whoever needs the app next.
 */
import { describe, expect, it, vi } from "vitest";
import { dismissOverlaysAside, dismissTransientOverlays, registerTransientOverlay } from "./overlays";

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

/**
 * A picker left open by one tour step must not sit over the next step's target:
 * the folder explorer over the Pipeline field, the skills picker over the Prompt
 * (FP of CORE-227). On a step change, the pickers the new target is not inside
 * are folded; the one it points into stays.
 */
describe("folding the pickers a new step does not point into", () => {
  it("closes a picker that does not hold the new target, and keeps the one that does", () => {
    const explorer = document.createElement("div");
    const row = document.createElement("button");
    explorer.append(row);
    const skills = document.createElement("div");
    document.body.append(explorer, skills);
    const closeExplorer = vi.fn();
    const closeSkills = vi.fn();
    const off = [
      registerTransientOverlay(closeExplorer, () => explorer),
      registerTransientOverlay(closeSkills, () => skills),
    ];

    dismissOverlaysAside([row]);

    expect(closeExplorer).not.toHaveBeenCalled();
    expect(closeSkills).toHaveBeenCalledTimes(1);
    for (const unregister of off) unregister();
    explorer.remove();
    skills.remove();
  });

  it("leaves alone an overlay that did not say where it is — only a tour start clears those", () => {
    const viewer = vi.fn();
    const off = registerTransientOverlay(viewer);

    dismissOverlaysAside([]);

    expect(viewer).not.toHaveBeenCalled();
    off();
  });
});
