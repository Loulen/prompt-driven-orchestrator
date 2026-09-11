/**
 * WCAG 2.1 contrast ratio (#759).
 *
 * The light theme is only worth shipping if it reads as well as the dark one, so
 * the ratio is computed, not eyeballed: `index.css.contrast.test.ts` asserts the
 * AA floors on BOTH palettes, and a future palette tweak that dips below them
 * fails the suite instead of shipping.
 *
 * Thresholds (WCAG 2.1 AA / RGAA): 4.5:1 body text, 3:1 large text and the
 * non-text parts of a component (borders, icons, focus rings).
 */

export const AA_TEXT = 4.5;
export const AA_LARGE_TEXT = 3;
export const AA_NON_TEXT = 3;

const HEX = /^#(?:[0-9a-f]{3}|[0-9a-f]{6})$/i;

function channels(hex: string): [number, number, number] {
  if (!HEX.test(hex)) throw new Error(`not a hex colour: ${hex}`);
  const body = hex.slice(1);
  const full =
    body.length === 3
      ? body
          .split("")
          .map((c) => c + c)
          .join("")
      : body;
  return [0, 2, 4].map((i) => parseInt(full.slice(i, i + 2), 16) / 255) as [number, number, number];
}

/** WCAG relative luminance: sRGB channels linearised, then weighted. */
function relativeLuminance(hex: string): number {
  const [r, g, b] = channels(hex).map((c) =>
    c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4,
  );
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

/** Contrast ratio between two opaque colours, from 1 (identical) to 21. */
export function contrastRatio(a: string, b: string): number {
  const [lighter, darker] = [relativeLuminance(a), relativeLuminance(b)].sort((x, y) => y - x);
  return (lighter + 0.05) / (darker + 0.05);
}
