/**
 * « Colour-blind friendly status colours » (#1005, Settings › General ›
 * Accessibility) — an opt-in status palette for deutan / protan vision.
 *
 * Modelled on `lib/theme.ts`: a *presentation* preference, so it lives in the
 * `pdo.*` localStorage namespace of this browser (every Project, no daemon
 * round-trip), never in `instance_config` — two people on one daemon must not
 * impose their colours on each other. Read and write are both guarded: private
 * mode, a disabled or full store, or a value we cannot read all fall back to the
 * default palette, without an error.
 *
 * Off by default. It only swaps the `st-*` tokens (see the colour-blind blocks of
 * `index.css`): the Status icon's glyphs, the accent and the theme are untouched.
 */

export const COLOUR_BLIND_STATUS_STORAGE_KEY = "pdo.ui.colourBlindStatus";

/** The attribute on `<html>` the colour-blind blocks of `index.css` select on. */
export const STATUS_PALETTE_ATTRIBUTE = "data-status-palette";
export const COLOUR_BLIND_PALETTE = "colour-blind";

/** #1005 — the detail of the colour-blind option, shown on hover of its Info
 *  icon (Settings › General › Accessibility) rather than as permanent help text. */
export const COLOUR_BLIND_STATUS_HELP =
  "Swaps the status colours for a set that stays apart under red-green colour blindness (deuteranopia, protanopia): completed, failed and awaiting no longer look alike. Status shapes do not change. Applies to the light and the dark theme, in this browser only.";

/** Stored preference. Only an explicit `"true"` turns it on; absent, unknown or
 *  unreadable → off (the default palette). */
export function loadColourBlindStatus(): boolean {
  try {
    return localStorage.getItem(COLOUR_BLIND_STATUS_STORAGE_KEY) === "true";
  } catch {
    return false;
  }
}

export function saveColourBlindStatus(on: boolean): void {
  try {
    localStorage.setItem(COLOUR_BLIND_STATUS_STORAGE_KEY, on ? "true" : "false");
  } catch {
    // quota / disabled / private mode → in-memory only for this session
  }
}

/**
 * Paint the palette on `<html>`. The attribute is present only while the option
 * is on, so a page with no stored preference carries nothing — the same DOM the
 * inline guard of `index.html` leaves when it finds nothing to stamp.
 */
export function applyColourBlindStatus(
  on: boolean,
  root: HTMLElement = document.documentElement,
): void {
  if (on) root.setAttribute(STATUS_PALETTE_ATTRIBUTE, COLOUR_BLIND_PALETTE);
  else root.removeAttribute(STATUS_PALETTE_ATTRIBUTE);
}
