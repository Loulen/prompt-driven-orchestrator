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
/** The element a dialog closing hands the focus back to, and when. */
let givenTo: { el: Element; at: number } | null = null;

/**
 * The focus `opener` is about to get is given back by code: a dialog closing
 * hands it to whatever opened it. Without this, the opener's tooltip read it as
 * a keyboard focus and stayed up over the next thing on screen — the « New run »
 * bubble over the Run row a tour had just lit, after Launch closed New Run (FP
 * of CORE-227). Only that element's focus counts as given back: a focus moved
 * elsewhere just after still opens its tooltip.
 */
export function markFocusGivenBack(opener: Element | null | undefined): void {
  // A dialog opened with nothing focused had the body as its opener: the body
  // holds every element, so it would mark every focus as given back.
  const real = opener && opener !== document.body && opener !== document.documentElement;
  givenTo = real ? { el: opener, at: Date.now() } : null;
}

const within = (at: number | null): boolean => {
  if (at === null) return false;
  const since = Date.now() - at;
  return since >= 0 && since <= GIVEN_BACK;
};

/**
 * Whether the focus `target` gets now is one given back by code. A clock set
 * back (a test's fake timers) counts as long ago.
 */
export function givenBack(target: EventTarget | null): boolean {
  if (within(lastGiving)) return true;
  return givenTo != null && target instanceof Node && givenTo.el.contains(target) && within(givenTo.at);
}
