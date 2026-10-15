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

/** #1005: the opt-in « Colour-blind friendly status colours », one block per theme. */
const darkColourBlindOverrides = paletteOf(
  ':root[data-status-palette="colour-blind"]:not([data-theme="light"]) {',
);
const lightColourBlindOverrides = paletteOf(
  ':root[data-theme="light"][data-status-palette="colour-blind"] {',
);
const darkColourBlind = { ...dark, ...darkColourBlindOverrides };
const lightColourBlind = { ...light, ...lightColourBlindOverrides };

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

/**
 * #1005: the default light palette went back to 1.119.1, where completed IS the
 * accent green (#03714f). As TEXT it holds AA on every surface; on its own tint
 * laid over `bg-0` (the page ground — pills mostly sit on bg-1 / bg-2) it reads
 * 4.37:1. Declared, not hidden: the colour-blind palette, built for this check
 * (#998), holds it outright. A floor — raise it only by improving the colour.
 */
const LIGHT_TINT_DEBT: Record<string, number> = {
  "st-done": 4.37,
};

function worstRatio(palette: Record<string, string>, token: string): number {
  return Math.min(...SURFACES.map((surface) => contrastRatio(palette[token], palette[surface])));
}

/** `rgba(r, g, b, a)` laid over an opaque hex surface, as the browser paints it. */
function composite(rgba: string, surface: string): string {
  const match = rgba.match(/^rgba\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)\s*,\s*([\d.]+)\s*\)$/);
  if (!match) throw new Error(`not an rgba colour: ${rgba}`);
  const alpha = Number(match[4]);
  const under = [1, 3, 5].map((i) => parseInt(surface.slice(i, i + 2), 16));
  return `#${[1, 2, 3]
    .map((i, k) => Math.round(Number(match[i]) * alpha + under[k] * (1 - alpha)))
    .map((channel) => channel.toString(16).padStart(2, "0"))
    .join("")}`;
}

/**
 * A status pill paints `text-st-X` on `bg-st-X-bg`, a translucent tint of the
 * same hue laid on whatever surface holds the pill (#998). The tint darkens the
 * ground under the words, so the plain-surface check above is not enough.
 */
function worstOnOwnTint(palette: Record<string, string>, token: string): number {
  const tint = palette[`${token}-bg`];
  return Math.min(
    ...SURFACES.map((surface) => contrastRatio(palette[token], composite(tint, palette[surface]))),
  );
}

describe("light palette (#759)", () => {
  it("holds AA (4.5:1) for every colour that carries text", () => {
    const failures = TEXT.map((token) => [token, worstRatio(light, token)] as const)
      .filter(([, ratio]) => ratio < AA_TEXT)
      .map(([token, ratio]) => `${token}: ${ratio.toFixed(2)}`);
    expect(failures).toEqual([]);
  });

  it("holds AA (4.5:1) for status text on its own tint, over every surface (#998)", () => {
    const failures = TEXT.filter((token) => token.startsWith("st-") && `${token}-bg` in light)
      .map((token) => [token, worstOnOwnTint(light, token), LIGHT_TINT_DEBT[token] ?? AA_TEXT] as const)
      .filter(([, ratio, floor]) => Number(ratio.toFixed(2)) < floor)
      .map(([token, ratio, floor]) => `${token}: ${ratio.toFixed(2)} < ${floor}`);
    expect(failures).toEqual([]);
  });

  it("declares tint debt only for colours that are actually below AA (#1005)", () => {
    const nowFine = Object.keys(LIGHT_TINT_DEBT).filter(
      (token) => worstOnOwnTint(light, token) >= AA_TEXT,
    );
    expect(nowFine).toEqual([]);
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

/** Every status token that carries text (`text-st-*`), with or without a tint. */
const STATUS_TEXT = TEXT.filter((token) => token.startsWith("st-"));

function textFailures(palette: Record<string, string>, tokens: string[]): string[] {
  return tokens
    .map((token) => [token, worstRatio(palette, token)] as const)
    .filter(([, ratio]) => ratio < AA_TEXT)
    .map(([token, ratio]) => `${token}: ${ratio.toFixed(2)}`);
}

function tintFailures(palette: Record<string, string>, tokens: string[]): string[] {
  return tokens
    .filter((token) => `${token}-bg` in palette)
    .map((token) => [token, worstOnOwnTint(palette, token)] as const)
    .filter(([, ratio]) => ratio < AA_TEXT)
    .map(([token, ratio]) => `${token}: ${ratio.toFixed(2)}`);
}

describe("default status palette = 1.119.1, interrupted and stale apart (#1005)", () => {
  const STATUS_1191 = ["st-await", "st-done", "st-failed"];

  it("gives back the 1.119.1 done / failed / awaiting in both themes", () => {
    expect(STATUS_1191.map((token) => dark[token])).toEqual(["#f59e0b", "#10b981", "#ef4444"]);
    expect(STATUS_1191.map((token) => light[token])).toEqual(["#8f4506", "#03714f", "#b91c1c"]);
  });

  it("keeps completed on the accent green, as in 1.119.1", () => {
    expect(dark["st-done"]).toBe(dark.acc);
    expect(light["st-done"]).toBe(light.acc);
  });

  it("no longer paints interrupted or stale like awaiting", () => {
    for (const palette of [dark, light]) {
      expect(palette["st-interrupted"]).not.toBe(palette["st-await"]);
      expect(palette["st-stale"]).not.toBe(palette["st-await"]);
      expect(palette["st-stale"]).not.toBe(palette["st-interrupted"]);
    }
    expect([dark["st-stale"], dark["st-interrupted"]]).toEqual(["#a8a29e", "#fcd34d"]);
    expect([light["st-stale"], light["st-interrupted"]]).toEqual(["#57534e", "#713f12"]);
  });
});

describe("colour-blind status palette (#1005)", () => {
  it("is the #998 set in light and variant A in dark", () => {
    const keys = ["st-done", "st-failed", "st-await", "st-stale", "st-interrupted"];
    expect(keys.map((token) => lightColourBlind[token])).toEqual([
      "#006b64",
      "#7f1d1d",
      "#9f4500",
      "#7b573f",
      "#7b410f",
    ]);
    expect(keys.map((token) => darkColourBlind[token])).toEqual([
      "#99f6e4",
      "#fb7185",
      "#facc15",
      "#d4c4a8",
      "#fb923c",
    ]);
  });

  it("only moves status colours — never the accent, running or a surface", () => {
    for (const overrides of [darkColourBlindOverrides, lightColourBlindOverrides]) {
      const moved = Object.keys(overrides);
      expect(moved.every((token) => token.startsWith("st-"))).toBe(true);
      expect(moved).not.toContain("st-running");
    }
  });

  it("overrides the same tokens in both themes, so neither theme keeps a stray default", () => {
    expect(Object.keys(darkColourBlindOverrides).sort()).toEqual(
      Object.keys(lightColourBlindOverrides).sort(),
    );
  });

  it("derives every -bg / -hl / -border variant from its own hue", () => {
    for (const overrides of [darkColourBlindOverrides, lightColourBlindOverrides]) {
      for (const [token, value] of Object.entries(overrides)) {
        const base = token.replace(/-(bg|hl|border)$/, "");
        if (base === token) continue;
        const hex = overrides[base];
        const rgb = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16)).join(", ");
        expect({ token, value: value.startsWith(`rgba(${rgb},`) }).toEqual({ token, value: true });
      }
    }
  });

  it("holds AA (4.5:1) for status text on every surface, in both themes", () => {
    expect(textFailures(lightColourBlind, STATUS_TEXT)).toEqual([]);
    // Dark: the tokens the option moves. The ones it leaves alone (skipped…)
    // are the default palette's, held by the no-regression check above.
    const moved = STATUS_TEXT.filter((token) => token in darkColourBlindOverrides);
    expect(moved.length).toBeGreaterThanOrEqual(5);
    expect(textFailures(darkColourBlind, moved)).toEqual([]);
  });

  it("holds AA (4.5:1) for status text on its own tint, in both themes", () => {
    expect(tintFailures(lightColourBlind, STATUS_TEXT)).toEqual([]);
    expect(tintFailures(darkColourBlind, Object.keys(darkColourBlindOverrides))).toEqual([]);
  });
});
