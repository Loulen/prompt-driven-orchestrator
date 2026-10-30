/**
 * The palette is a contract, not a mood board (#759, ADR-0091).
 *
 * The shared token sheet (`public/shared/tokens.css`) defines the palette of
 * each Theme in each mode (ADR-0093). Read here, so the check cannot drift
 * from what ships:
 *  - each Theme × mode defines every colour of the palette, so no token is
 *    left to paint a dark island on a light page (or the reverse);
 *  - every ink that carries text holds the WCAG 2.1 AA floor (4.5:1) on every
 *    surface, in each of them;
 *  - the status colours and the accent, which carry icons and outlines but
 *    also words (an error line, a status label, a link), hold the same 4.5:1.
 */
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, it, expect } from "vitest";
import { contrastRatio, AA_TEXT } from "./lib/contrast";

const CSS = readFileSync(join(import.meta.dirname, "..", "public", "shared", "tokens.css"), "utf8");

/** Every `--color-*: <value>;` of the block that opens with `opener`. */
function paletteOf(opener: string): Record<string, string> {
  const start = CSS.indexOf(opener);
  if (start < 0) throw new Error(`block not found: ${opener}`);
  const end = CSS.indexOf("}", start);
  const out: Record<string, string> = {};
  for (const [, name, value] of CSS.slice(start, end).matchAll(/--color-([a-z0-9-]+)\s*:\s*([^;]+);/gi)) {
    out[name] = value.trim();
  }
  return out;
}

const PALETTES: [string, Record<string, string>][] = [
  ["Acogni dark", paletteOf(':root,\n[data-mode="dark"] {')],
  ["Acogni light", paletteOf('[data-mode="light"] {')],
  ["Classic dark", paletteOf('[data-theme="classic"],\n[data-theme="classic"] [data-mode="dark"] {')],
  ["Classic light", paletteOf('[data-theme="classic"][data-mode="light"],\n[data-theme="classic"] [data-mode="light"] {')],
];

const SURFACES = ["paper", "paper-2", "paper-3"];
const TEXT = ["ink", "ink-2", "muted"];
const TONES = ["accent", "st-running", "st-await", "st-done", "st-blocked", "st-failed", "st-skipped"];

function worst(palette: Record<string, string>, token: string): number {
  return Math.min(...SURFACES.map((surface) => contrastRatio(palette[token], palette[surface])));
}

describe.each(PALETTES)("%s palette", (_, palette) => {
  it("holds AA (4.5:1) for every ink that carries text", () => {
    const failures = TEXT.filter((token) => worst(palette, token) < AA_TEXT).map(
      (token) => `${token}: ${worst(palette, token).toFixed(2)}`,
    );
    expect(failures).toEqual([]);
  });

  it("holds AA (4.5:1) for the accent and the status colours, which carry words too", () => {
    const failures = TONES.filter((token) => worst(palette, token) < AA_TEXT).map(
      (token) => `${token}: ${worst(palette, token).toFixed(2)}`,
    );
    expect(failures).toEqual([]);
  });

  it("writes legible text on the accent (a primary button)", () => {
    expect(contrastRatio(palette["accent-ink"], palette.accent)).toBeGreaterThanOrEqual(AA_TEXT);
  });
});

describe("every Theme × mode", () => {
  it("defines the same colours", () => {
    const [, reference] = PALETTES[0];
    for (const [, palette] of PALETTES) expect(Object.keys(palette).sort()).toEqual(Object.keys(reference).sort());
  });
});
