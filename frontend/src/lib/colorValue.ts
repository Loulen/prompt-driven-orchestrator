/**
 * Read a CSS colour as sRGB channels (ADR-0091).
 *
 * The palette of the shared design system is written in OKLCH; the places
 * that need a resolved colour (a contrast ratio, mermaid's SVG, a chart's
 * props) take sRGB. Handles what the palette and a browser's computed style
 * produce: `#rgb`, `#rrggbb`, `rgb()` / `rgba()`, `oklch()` and
 * `color(srgb …)`. Anything else gives `null`.
 */

export interface Rgba {
  /** 0 to 1, gamma-encoded sRGB. */
  r: number;
  g: number;
  b: number;
  /** 0 to 1. */
  a: number;
}

const clamp01 = (v: number) => Math.min(1, Math.max(0, v));

function number(token: string, percentScale = 1): number {
  const t = token.trim();
  if (t.endsWith("%")) return (Number.parseFloat(t) / 100) * percentScale;
  return Number.parseFloat(t);
}

/** `a b c / d` or `a, b, c, d` → the four tokens (alpha may be missing). */
function args(body: string): string[] {
  return body
    .replace(/\s*\/\s*/, " ")
    .replace(/,/g, " ")
    .trim()
    .split(/\s+/);
}

function fromHex(hex: string): Rgba | null {
  const body = hex.slice(1);
  if (!/^(?:[0-9a-f]{3}|[0-9a-f]{6})$/i.test(body)) return null;
  const full = body.length === 3 ? [...body].map((c) => c + c).join("") : body;
  const [r, g, b] = [0, 2, 4].map((i) => Number.parseInt(full.slice(i, i + 2), 16) / 255);
  return { r, g, b, a: 1 };
}

const encode = (c: number) => (c <= 0.0031308 ? 12.92 * c : 1.055 * c ** (1 / 2.4) - 0.055);

/** OKLCH (L 0..1, C, H degrees) to gamma-encoded sRGB, clipped to the gamut. */
export function oklchToSrgb(l: number, c: number, h: number): [number, number, number] {
  const rad = (h * Math.PI) / 180;
  const a = c * Math.cos(rad);
  const b = c * Math.sin(rad);
  const l1 = (l + 0.3963377774 * a + 0.2158037573 * b) ** 3;
  const m1 = (l - 0.1055613458 * a - 0.0638541728 * b) ** 3;
  const s1 = (l - 0.0894841775 * a - 1.291485548 * b) ** 3;
  const r = 4.0767416621 * l1 - 3.3077115913 * m1 + 0.2309699292 * s1;
  const g = -1.2684380046 * l1 + 2.6097574011 * m1 - 0.3413193965 * s1;
  const bl = -0.0041960863 * l1 - 0.7034186147 * m1 + 1.707614701 * s1;
  return [clamp01(encode(r)), clamp01(encode(g)), clamp01(encode(bl))];
}

/** The colour `value` as sRGB, or `null` when it is not one of the forms above. */
export function parseColor(value: string): Rgba | null {
  const v = value.trim().toLowerCase();
  if (v.startsWith("#")) return fromHex(v);
  const fn = /^([a-z]+)\((.*)\)$/.exec(v);
  if (!fn) return null;
  const [, name, body] = fn;
  const parts = args(body);
  const alpha = parts.length > 3 && name !== "color" ? number(parts[3], 1) : 1;
  if (name === "rgb" || name === "rgba") {
    const [r, g, b] = parts.slice(0, 3).map((p) => number(p, 255) / 255);
    return { r, g, b, a: alpha };
  }
  if (name === "oklch") {
    const [l, c, h] = [number(parts[0], 1), number(parts[1], 0.4), Number.parseFloat(parts[2]) || 0];
    const [r, g, b] = oklchToSrgb(l, c, h);
    return { r, g, b, a: alpha };
  }
  if (name === "color" && parts[0] === "srgb") {
    const [r, g, b] = parts.slice(1, 4).map((p) => number(p, 1));
    const a = parts.length > 4 ? number(parts[4], 1) : 1;
    return { r, g, b, a };
  }
  return null;
}

/** `#rrggbb`, or `rgba(…)` when the colour is translucent. */
export function formatColor({ r, g, b, a }: Rgba): string {
  const byte = (c: number) => Math.round(clamp01(c) * 255);
  if (a < 1) return `rgba(${byte(r)}, ${byte(g)}, ${byte(b)}, ${Number(a.toFixed(3))})`;
  return `#${[r, g, b].map((c) => byte(c).toString(16).padStart(2, "0")).join("")}`;
}
