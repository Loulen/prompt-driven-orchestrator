/**
 * Read a palette token at runtime (#759).
 *
 * Most of the UI paints through Tailwind classes, which resolve `var(--color-*)`
 * in CSS and re-theme for free. Three surfaces cannot: mermaid bakes hex values
 * into the SVG's own `<style>`, xterm paints on a canvas, and recharts takes
 * colours as props. Those read the live token through here instead of freezing
 * a hex, so one palette stays the single source of truth.
 *
 * `fallback` covers the non-browser case (SSR, a test with no stylesheet) — an
 * unresolvable token must never yield `""`, which paints black.
 */
export function cssColor(token: `--color-${string}`, fallback: string): string {
  if (typeof getComputedStyle !== "function") return fallback;
  const value = getComputedStyle(document.documentElement).getPropertyValue(token).trim();
  return value || fallback;
}
