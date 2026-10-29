import { formatColor, parseColor } from "./colorValue";

/**
 * Read a token of the design system at runtime, resolved (#759, ADR-0091).
 *
 * Most of the UI paints through Tailwind classes, which resolve `var(--…)` in
 * CSS and re-theme for free. Three surfaces cannot: mermaid bakes colours into
 * the SVG's own `<style>`, xterm paints on a canvas, and recharts takes
 * colours as props. Those read the live token through here instead of freezing
 * a value, so one palette stays the single source of truth.
 *
 * The palette is OKLCH and a role may be a `color-mix()`; the colour comes back
 * as `#rrggbb` (or `rgba()` when translucent), which every consumer parses.
 * An unresolvable token (no stylesheet, a test) gives `fallback`, a neutral
 * grey by default: never `""`, which paints black.
 */
export function cssColor(token: `--${string}`, fallback = "rgb(128, 128, 128)"): string {
  if (typeof getComputedStyle !== "function" || typeof document === "undefined") return fallback;
  const root = document.documentElement;
  const raw = getComputedStyle(root).getPropertyValue(token).trim();
  if (!raw) return fallback;
  const direct = parseColor(raw);
  if (direct) return formatColor(direct);
  // A role that mixes colours: let the engine compute it on a probe.
  const probe = document.createElement("span");
  probe.style.color = `var(${token})`;
  root.appendChild(probe);
  const computed = getComputedStyle(probe).color;
  probe.remove();
  const resolved = parseColor(computed);
  return resolved ? formatColor(resolved) : fallback;
}

/**
 * Read a non-colour token resolved (a length of the type scale), or `fallback`
 * when there is no stylesheet. For the surfaces that take numbers rather than
 * CSS: mermaid's label size, the terminal's grid.
 */
export function cssToken(token: `--${string}`, fallback: string): string {
  if (typeof getComputedStyle !== "function" || typeof document === "undefined") return fallback;
  return getComputedStyle(document.documentElement).getPropertyValue(token).trim() || fallback;
}
