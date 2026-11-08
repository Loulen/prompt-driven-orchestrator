/**
 * The Palette daltonienne (CONTEXT.md): « Colour-blind friendly status
 * colours » of Settings › Appearance, off by default, a field of the Actor's
 * Apparence (ADR-0096). On, `<html>` wears `data-status-palette="colour-blind"`
 * and `/shared/tokens.css` swaps the status colours of every Theme, light and
 * dark, for a set that stays apart under red-green colour blindness. Only the
 * colours change: the Status icon keeps its shape.
 *
 * This browser keeps a copy under {@link STATUS_PALETTE_STORAGE_KEY}, so the
 * theme script (`public/shared/theme.js`) stamps the attribute before the first
 * paint, in the interface and on a page an extension serves outside it. Only a
 * stored `"true"` turns it on; `noFlashTheme.test.ts` holds the two together.
 */

export const STATUS_PALETTE_STORAGE_KEY = "pdo.ui.colourBlindStatus";

/** The attribute on `<html>` that selects the palette, and its one value. */
export const STATUS_PALETTE_ATTRIBUTE = "data-status-palette";
export const COLOUR_BLIND = "colour-blind";

/** The label of the switch, and its tooltip. */
export const COLOUR_BLIND_STATUS_LABEL = "Colour-blind friendly status colours";
export const COLOUR_BLIND_STATUS_HELP =
  "Swaps the status colours for a set that stays apart under red-green colour blindness (deuteranopia, protanopia): completed, failed and awaiting no longer look alike. Status shapes do not change. Applies to every Theme, light and dark, on every device you sign in from.";

/** The stored copy; absent, unknown or unreadable → off. */
export function loadColourBlindStatus(): boolean {
  try {
    return localStorage.getItem(STATUS_PALETTE_STORAGE_KEY) === "true";
  } catch {
    return false;
  }
}

export function saveColourBlindStatus(on: boolean): void {
  try {
    if (on) localStorage.setItem(STATUS_PALETTE_STORAGE_KEY, "true");
    else localStorage.removeItem(STATUS_PALETTE_STORAGE_KEY);
  } catch {
    // quota / disabled / private mode → in-memory only for this session
  }
}

/** Paint the palette on `<html>`: the attribute on, or gone. */
export function applyStatusPalette(on: boolean, root: HTMLElement = document.documentElement): void {
  if (on) root.setAttribute(STATUS_PALETTE_ATTRIBUTE, COLOUR_BLIND);
  else root.removeAttribute(STATUS_PALETTE_ATTRIBUTE);
}
