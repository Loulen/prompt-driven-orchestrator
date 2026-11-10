import { parseColor } from "./colorValue";

/**
 * WCAG 2.1 contrast ratio (#759).
 *
 * The light theme is only worth shipping if it reads as well as the dark one, so
 * the ratio is computed, not eyeballed: `themePalette.test.ts` asserts the
 * AA floors on BOTH palettes, and a future palette tweak that dips below them
 * fails the suite instead of shipping.
 *
 * Colours in any form `parseColor` reads (hex, rgb, OKLCH).
 *
 * Thresholds (WCAG 2.1 AA / RGAA): 4.5:1 body text, 3:1 large text and the
 * non-text parts of a component (borders, icons, focus rings).
 */

export const AA_TEXT = 4.5;
export const AA_LARGE_TEXT = 3;
export const AA_NON_TEXT = 3;

function channels(color: string): [number, number, number] {
  const rgba = parseColor(color);
  if (!rgba || rgba.a < 1) throw new Error(`not an opaque colour: ${color}`);
  return [rgba.r, rgba.g, rgba.b];
}

/** WCAG relative luminance: sRGB channels linearised, then weighted. */
function relativeLuminance(color: string): number {
  const [r, g, b] = channels(color).map((c) =>
    c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4,
  );
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

/** Contrast ratio between two opaque colours, from 1 (identical) to 21. */
export function contrastRatio(a: string, b: string): number {
  const [lighter, darker] = [relativeLuminance(a), relativeLuminance(b)].sort((x, y) => y - x);
  return (lighter + 0.05) / (darker + 0.05);
}
