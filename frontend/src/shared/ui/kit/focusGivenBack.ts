/**
 * Whether a focus is one **given back by code** — a menu closed or picked from
 * returning it to its trigger, a dialog closing and handing it to its opener —
 * rather than one a keyboard user moved. A tooltip opens on the second only.
 * Its own module so `tip.tsx` exports components alone (fast refresh).
 */

/**
 * A focus given back by code within this time after Escape, or after Enter,
 * Space or a pointer-down in an open layer (a menu closed or picked from
 * returns to its trigger), opens no tooltip; Tab and the arrows do.
 */
const GIVEN_BACK = 500;
const LAYER = '.d-pop, [role="menu"], [role="listbox"]';
let lastGiving: number | null = null;
const inLayer = (t: EventTarget | null) => t instanceof Element && !!t.closest(LAYER);
if (typeof window !== "undefined") {
  window.addEventListener(
    "keydown",
    (e) => (lastGiving = e.key === "Escape" || ((e.key === "Enter" || e.key === " ") && inLayer(e.target)) ? Date.now() : null),
    true,
  );
  window.addEventListener(
    "pointerdown",
    (e) => {
      if (inLayer(e.target)) lastGiving = Date.now();
    },
    true,
  );
}
/**
 * The focus about to move is given back by code: a dialog closing hands it to
 * whatever opened it. Without this, the opener's tooltip read it as a keyboard
 * focus and stayed up over the next thing on screen — the « New run » bubble
 * over the Run row a tour had just lit, after Launch closed New Run (FP of
 * CORE-227).
 */
export function markFocusGivenBack(): void {
  lastGiving = Date.now();
}

/** Whether a focus now is one given back by code. A clock set back (a test's fake timers) counts as long ago. */
export function givenBack(): boolean {
  if (lastGiving === null) return false;
  const since = Date.now() - lastGiving;
  return since >= 0 && since <= GIVEN_BACK;
}
