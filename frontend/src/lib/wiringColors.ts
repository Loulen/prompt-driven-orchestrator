/**
 * The wiring palette (#844). Amber all the way through — the same family the
 * selected edge already uses (`--color-edge-selected`), so « this is wiring » is
 * one colour story from the rim glow to the preview path to the grid to the
 * drop-target ring. Green is the app's accent and already means something else on
 * a card.
 */
export const WIRE = "var(--color-st-await)";
export const WIRE_SOFT = "color-mix(in srgb, var(--color-st-await) 45%, transparent)";
export const WIRE_BG = "color-mix(in srgb, var(--color-st-await) 14%, transparent)";
