/**
 * The palette is a contract, not a mood board (#759).
 *
 * Two properties, not one:
 *  - the LIGHT theme, which is new, holds the WCAG 2.1 AA floors outright;
 *  - the DARK theme, which shipped long before this ticket, may not regress.
 *    Several of its greys sit below AA today (`fg-4` carries captions at
 *    2.40:1). Raising them restyles hundreds of screens, which is its own
 *    ticket, so the debt is DECLARED here rather than silently accepted: each
 *    entry is the ratio measured today, and the dark theme can only improve.
 *
 * And the light theme must cover EVERY colour the dark one declares — a token
 * left behind would paint a dark island on a light page.
 *
 * The source of truth is `index.css` itself, read here, so the check cannot
 * drift from what actually ships.
 */
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, it, expect } from "vitest";
import { contrastRatio, AA_TEXT, AA_NON_TEXT } from "./lib/contrast";

const CSS = readFileSync(join(import.meta.dirname, "index.css"), "utf8");

/** Every `--color-*: <value>;` inside the block that starts at `opener`. */
function paletteOf(opener: string): Record<string, string> {
  const start = CSS.indexOf(opener);
  if (start < 0) throw new Error(`block not found: ${opener}`);
  let depth = 0;
  let end = start;
  for (let i = CSS.indexOf("{", start); i < CSS.length; i++) {
    if (CSS[i] === "{") depth++;
    else if (CSS[i] === "}" && --depth === 0) {
      end = i;
      break;
    }
  }
  const out: Record<string, string> = {};
  for (const [, name, value] of CSS.slice(start, end).matchAll(
    /--color-([a-z0-9-]+)\s*:\s*([^;]+);/gi,
  )) {
    out[name] = value.trim();
  }
  return out;
}

const darkOverrides = paletteOf("@theme {");
const lightOverrides = paletteOf(':root[data-theme="light"] {');
const dark = darkOverrides;
const light = { ...darkOverrides, ...lightOverrides };

/** Surfaces that carry copy. bg-4 / bg-5 are hover and pressed states. */
const SURFACES = ["bg-0", "bg-1", "bg-2", "bg-3"];

/** Tokens a component paints TEXT with — `text-<token>` appears in `src/`. */
const TEXT = [
  "fg",
  "fg-2",
  "fg-3",
  "fg-4",
  "acc",
  "st-running",
  "st-await",
  "st-done",
  "st-blocked",
  "st-failed",
  "st-skipped",
  "st-stopped",
  "st-stale",
  "st-interrupted",
  "st-paused",
  "edit-tint",
  "edge-selected",
  "chart-axis",
];

/**
 * Tokens that only ever fill or outline — hover fills, status dots — so they owe
 * the 3:1 non-text floor. Promote one to TEXT the day a component prints words
 * in it (`grep -rn "text-<token>" src` finds nothing for any of these today).
 *
 * `fg-5` is in neither list: it IS used as text, at a ratio no five-step grey
 * ramp can hold. A defect of the type scale, not of the palette — out of scope
 * here, and not worth a fake assertion.
 */
const NON_TEXT = [
  "acc-dim",
  "st-archived",
  "st-pending",
  "chart-runs",
  "chart-errors",
  "chart-fires",
];

/**
 * The dark theme's pre-existing sub-AA tokens, with the ratio measured the day
 * #759 landed. A floor, not a target: lower this number only by improving the
 * colour, never to make a regression pass.
 */
const DARK_DEBT: Record<string, number> = {
  "fg-3": 4.08,
  "fg-4": 2.4,
  "st-failed": 4.44,
  "st-skipped": 3.51,
  "st-pending": 2.21,
};

function worstRatio(palette: Record<string, string>, token: string): number {
  return Math.min(...SURFACES.map((surface) => contrastRatio(palette[token], palette[surface])));
}

describe("light palette (#759)", () => {
  it("holds AA (4.5:1) for every colour that carries text", () => {
    const failures = TEXT.map((token) => [token, worstRatio(light, token)] as const)
      .filter(([, ratio]) => ratio < AA_TEXT)
      .map(([token, ratio]) => `${token}: ${ratio.toFixed(2)}`);
    expect(failures).toEqual([]);
  });

  it("holds the 3:1 non-text floor for every fill-and-outline colour", () => {
    const failures = NON_TEXT.map((token) => [token, worstRatio(light, token)] as const)
      .filter(([, ratio]) => ratio < AA_NON_TEXT)
      .map(([token, ratio]) => `${token}: ${ratio.toFixed(2)}`);
    expect(failures).toEqual([]);
  });

  it("overrides every colour token the base palette declares", () => {
    const missing = Object.keys(dark).filter((token) => !(token in lightOverrides));
    expect(missing).toEqual([]);
  });
});

describe("dark palette — no regression (#759)", () => {
  it("keeps every colour at AA, or at least at its declared debt floor", () => {
    const failures = [...TEXT, ...NON_TEXT]
      .map((token) => {
        const floor = DARK_DEBT[token] ?? (TEXT.includes(token) ? AA_TEXT : AA_NON_TEXT);
        return [token, worstRatio(dark, token), floor] as const;
      })
      .filter(([, ratio, floor]) => ratio < floor)
      .map(([token, ratio, floor]) => `${token}: ${ratio.toFixed(2)} < ${floor}`);
    expect(failures).toEqual([]);
  });

  it("declares debt only for colours that are actually below AA", () => {
    const nowFine = Object.keys(DARK_DEBT).filter((token) => worstRatio(dark, token) >= AA_TEXT);
    expect(nowFine).toEqual([]);
  });
});
