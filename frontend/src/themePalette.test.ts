/**
 * The palette is a contract, not a mood board (#759, ADR-0091).
 *
 * The shared token sheet (`public/shared/tokens.css`) defines the palette of
 * both themes. Read here, so the check cannot drift from what ships:
 *  - each theme defines every colour of the palette, so no token is left to
 *    paint a dark island on a light page (or the reverse);
 *  - every ink that carries text holds the WCAG 2.1 AA floor (4.5:1) on every
 *    surface, in both themes;
 *  - the status colours and the ruby, which carry icons and outlines but
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

const DARK = paletteOf(':root,\n[data-theme="dark"] {');
const LIGHT = paletteOf('[data-theme="light"] {');

const SURFACES = ["paper", "paper-2", "paper-3"];
const TEXT = ["ink", "ink-2", "muted"];
const TONES = ["accent", "st-running", "st-await", "st-done", "st-blocked", "st-failed", "st-skipped"];

function worst(palette: Record<string, string>, token: string): number {
  return Math.min(...SURFACES.map((surface) => contrastRatio(palette[token], palette[surface])));
}

describe.each([
  ["dark", DARK],
  ["light", LIGHT],
])("%s palette", (_, palette) => {
  it("holds AA (4.5:1) for every ink that carries text", () => {
    const failures = TEXT.filter((token) => worst(palette, token) < AA_TEXT).map(
      (token) => `${token}: ${worst(palette, token).toFixed(2)}`,
    );
    expect(failures).toEqual([]);
  });

  it("holds AA (4.5:1) for the ruby and the status colours, which carry words too", () => {
    const failures = TONES.filter((token) => worst(palette, token) < AA_TEXT).map(
      (token) => `${token}: ${worst(palette, token).toFixed(2)}`,
    );
    expect(failures).toEqual([]);
  });

  it("writes legible text on the ruby (a primary button)", () => {
    expect(contrastRatio(palette["accent-ink"], palette.accent)).toBeGreaterThanOrEqual(AA_TEXT);
  });
});

describe("both themes", () => {
  it("define the same colours", () => {
    expect(Object.keys(LIGHT).sort()).toEqual(Object.keys(DARK).sort());
  });
});
