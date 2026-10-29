// CORE-124 — the Zoom du terminal of a Session hébergée (CONTEXT.md): the size
// of its terminal's text, from 50 % to 200 %, the same for every Poste on it.
// An extension's host may impose a zoom of its own on top (a surface it scales,
// a thumbnail): the two multiply.

/** The steps the zoom control offers, in percent. */
export const TERMINAL_ZOOMS = [50, 75, 100, 125, 150, 200] as const;
/** The zoom of a session nobody changed, and of any session that is not hosted. */
export const DEFAULT_TERMINAL_ZOOM = 100;
export const MIN_TERMINAL_ZOOM = 50;
export const MAX_TERMINAL_ZOOM = 200;

/** `zoom` kept within the range; a value that is no number is the default. */
export function clampTerminalZoom(zoom: number): number {
  if (!Number.isFinite(zoom)) return DEFAULT_TERMINAL_ZOOM;
  return Math.min(MAX_TERMINAL_ZOOM, Math.max(MIN_TERMINAL_ZOOM, Math.round(zoom)));
}

/**
 * What the terminal's font is multiplied by: the session's zoom (percent) times
 * the zoom its host imposes (a factor, `1` by default). A host zoom that is no
 * positive number counts as `1`.
 */
export function terminalFontFactor(sessionZoom: number, hostZoom = 1): number {
  const host = Number.isFinite(hostZoom) && hostZoom > 0 ? hostZoom : 1;
  return (clampTerminalZoom(sessionZoom) / 100) * host;
}

/**
 * The step after `zoom` toward `direction` (`1`: bigger, `-1`: smaller): the
 * next of {@link TERMINAL_ZOOMS}, or `zoom` itself at either end. A zoom
 * between two steps goes to the nearest one in that direction.
 */
export function stepTerminalZoom(zoom: number, direction: 1 | -1): number {
  const current = clampTerminalZoom(zoom);
  if (direction > 0) return TERMINAL_ZOOMS.find((z) => z > current) ?? current;
  return [...TERMINAL_ZOOMS].reverse().find((z) => z < current) ?? current;
}
