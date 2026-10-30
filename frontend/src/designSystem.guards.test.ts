/**
 * S4, S5 — the guards of the shared design system (ADR-0091, ADR-0093).
 *
 *  - Each Theme defines the whole palette in each mode, without a hex colour,
 *    and the roles read it.
 *  - Nothing reads `data-theme` in its sense before 2.68 (the luminosity).
 *  - The old token names are gone, everywhere: no alias.
 *  - No font size in pixels and no hex colour in the source.
 *  - No font is fetched from a third party: the binary serves them.
 */
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import { describe, expect, it } from "vitest";

const SRC = import.meta.dirname;
const FRONT = join(SRC, "..");
const TOKENS = readFileSync(join(FRONT, "public", "shared", "tokens.css"), "utf8");
const INDEX_CSS = readFileSync(join(SRC, "index.css"), "utf8");
const BRAND_CSS = readFileSync(join(SRC, "shared", "ui", "brand", "brand.css"), "utf8");

function walk(dir: string, keep: (path: string) => boolean): string[] {
  return readdirSync(dir).flatMap((entry) => {
    const path = join(dir, entry);
    if (statSync(path).isDirectory()) return entry === "generated" ? [] : walk(path, keep);
    return keep(path) ? [path] : [];
  });
}

/** The front's source, tests excluded. */
const SOURCE = walk(SRC, (p) => /\.(tsx?|css)$/.test(p) && !p.includes(".test."));
/** The source, the tests, the end-to-end specs, the shared module and the test extensions' bundles. */
const EVERYTHING = [
  ...walk(SRC, (p) => /\.(tsx?|css)$/.test(p)),
  ...walk(join(FRONT, "e2e"), (p) => /\.ts$/.test(p)),
  ...walk(join(FRONT, "public", "shared"), (p) => /\.(js|d\.ts|css)$/.test(p)),
  ...walk(join(FRONT, "..", "crates", "pdo", "tests", "test_extension"), (p) => /\.(js|html)$/.test(p)),
];

/** The custom properties a CSS block of `css` declares. */
function declared(opener: string, css = TOKENS): Set<string> {
  const start = css.indexOf(opener);
  if (start < 0) throw new Error(`block not found: ${opener}`);
  const end = css.indexOf("}", start);
  return new Set([...css.slice(start, end).matchAll(/(--[\w-]+)\s*:/g)].map((m) => m[1]));
}

/** The value of `property` in the block that opens with `opener`. */
function valueIn(css: string, opener: string, property: string): string {
  const start = css.indexOf(opener);
  if (start < 0) throw new Error(`block not found: ${opener}`);
  const block = css.slice(start, css.indexOf("}", start));
  const match = block.match(new RegExp(`${property}\\s*:\\s*([^;]+);`));
  if (!match) throw new Error(`${property} not in ${opener}`);
  return match[1].trim();
}

/** The palette blocks, one per Theme × mode. */
const PALETTES = {
  "acogni dark": ':root,\n[data-mode="dark"] {',
  "acogni light": '[data-mode="light"] {',
  "classic dark": '[data-theme="classic"],\n[data-theme="classic"] [data-mode="dark"] {',
  "classic light": '[data-theme="classic"][data-mode="light"],\n[data-theme="classic"] [data-mode="light"] {',
} as const;
const ROLES = ":root,\n[data-theme],\n[data-mode] {";

describe("tokens", () => {
  it.each(Object.entries(PALETTES))("define the whole palette under %s", (_, opener) => {
    const reference = declared(PALETTES["acogni dark"]);
    expect(reference.has("--color-paper")).toBe(true);
    expect([...declared(opener)].sort()).toEqual([...reference].sort());
  });

  it("add no token of their own to a Theme: the roles are declared once", () => {
    const roles = declared(ROLES);
    for (const opener of Object.values(PALETTES)) {
      expect([...declared(opener)].filter((name) => roles.has(name))).toEqual([]);
    }
  });

  it("write no hex colour: the palette is OKLCH", () => {
    expect(TOKENS.match(HEX) ?? []).toEqual([]);
  });

  it("define every role Tailwind's utilities paint with", () => {
    const roles = declared(ROLES);
    const palette = declared(PALETTES["acogni dark"]);
    const theme = INDEX_CSS.slice(INDEX_CSS.indexOf("@theme inline reference {"));
    const used = [...theme.slice(0, theme.indexOf("\n}")).matchAll(/--color-[\w-]+:\s*var\((--[\w-]+)\)/g)].map((m) => m[1]);
    expect(used.length).toBeGreaterThan(20);
    expect(used.filter((name) => !roles.has(name) && !palette.has(name))).toEqual([]);
  });

  it("keep the logo in the Acogni ruby of each mode under Classic", () => {
    expect(valueIn(BRAND_CSS, '[data-theme="classic"] .acogni-brand {', "--primary")).toBe(
      valueIn(TOKENS, PALETTES["acogni dark"], "--color-accent"),
    );
    expect(valueIn(BRAND_CSS, '[data-theme="classic"] [data-mode="light"] .acogni-brand {', "--primary")).toBe(
      valueIn(TOKENS, PALETTES["acogni light"], "--color-accent"),
    );
  });
});

/** `data-theme` in its sense before 2.68: dark, light or system (now `data-mode`). */
const OLD_THEME_ATTRIBUTE = /data-theme\W{0,8}(?:dark|light|system)\b/g;

describe("the attribute data-theme (ADR-0093)", () => {
  it("never carries the mode: source, tests, end-to-end specs, shared module, test extensions, reference", () => {
    const self = join(SRC, "designSystem.guards.test.ts");
    const files = [
      ...EVERYTHING,
      join(FRONT, "index.html"),
      join(FRONT, "..", "docs", "reference", "extension-ui.md"),
      join(FRONT, "..", "docs", "reference", "extension-api.md"),
    ];
    const offenders = files
      .filter((file) => file !== self)
      .flatMap((file) =>
        (readFileSync(file, "utf8").match(OLD_THEME_ATTRIBUTE) ?? []).map((hit) => `${relative(FRONT, file)}: ${hit}`),
      );
    expect(offenders).toEqual([]);
  });
});

/** The old palette: utilities and custom properties (ADR-0091: no alias). */
const OLD_NAMES = new RegExp(
  [
    // `bg-bg-3`, `text-fg-4`, `border-acc-border`, `bg-st-failed-bg`…
    String.raw`(?<![\w-])(?:bg|text|border(?:-[trblxyse])?|ring|fill|stroke|outline|from|to|via|divide|shadow|decoration|placeholder|caret|accent)-(?:bg(?:-[0-5])?|fg(?:-[2-5])?|acc(?:-dim|-bg|-border|-hi)?|on-acc(?:-warn)?|edit-tint|edge-selected|tour-dim(?:-soft)?|chart-[a-z-]+|st-(?:stopped|archived|stale|pending|interrupted|paused)|st-[a-z]+-(?:bg|hl|border))(?![\w-])`,
    // `--color-bg-2`, `--color-fg`, `--color-acc`, `--color-st-await-bg`…
    String.raw`--color-(?:bg-[0-5]|fg(?:-[2-5])?|acc|acc-[\w-]+|on-acc[\w-]*|edit-tint|edge-selected|shadow|tour-dim[\w-]*|chart-[\w-]+|st-(?:stopped|archived|stale|pending|interrupted|paused)|st-[a-z]+-(?:bg|hl|border))(?![\w-])`,
    String.raw`var\(--font-(?:sans|mono)\)`,
  ].join("|"),
  "g",
);

describe("the old token names", () => {
  it("appear nowhere: source, tests, end-to-end specs, shared module, test extensions", () => {
    const self = join(SRC, "designSystem.guards.test.ts");
    const offenders = EVERYTHING.filter((file) => file !== self).flatMap((file) =>
      (readFileSync(file, "utf8").match(OLD_NAMES) ?? []).map((hit) => `${relative(FRONT, file)}: ${hit}`),
    );
    expect(offenders).toEqual([]);
  });
});

/** A font size in pixels: an inline `fontSize`, a `text-[11px]`, a CSS `font-size: 11px`. */
const PIXEL_FONT_SIZE = /\bfontSize\s*[:,}]|(?<![\w-])text-\[[\d.]+(?:px|rem)\]|font-size\s*:\s*[\d.]+(?:px|rem)/;
/** A hex colour (an issue number such as `#759` is not one). */
const HEX = /(?<![\w&/])#([0-9a-fA-F]{8}|[0-9a-fA-F]{6}|[0-9a-fA-F]{3,4})\b/g;

function unmigrated(source: string): boolean {
  if (PIXEL_FONT_SIZE.test(source)) return true;
  return [...source.matchAll(HEX)].some(([, hex]) => /[a-f]/i.test(hex) || hex.length >= 6);
}

describe("the type scale and the palette in the source", () => {
  it("hold in every file: sizes from the scale, colours from the roles", () => {
    expect(SOURCE.filter((file) => unmigrated(readFileSync(file, "utf8"))).map((file) => relative(SRC, file))).toEqual([]);
  });
});

describe("fonts", () => {
  const FILES = [
    join(FRONT, "index.html"),
    join(FRONT, "public", "shared", "tokens.css"),
    ...walk(SRC, (p) => p.endsWith(".css")),
  ];

  it("are never fetched from a third party", () => {
    const external = /(?:@import\s+url\(|url\(|href=)\s*["']?(?:https?:)?\/\//g;
    const offenders = FILES.flatMap((file) =>
      (readFileSync(file, "utf8").match(external) ?? []).map((hit) => `${relative(FRONT, file)}: ${hit}`),
    );
    expect(offenders).toEqual([]);
  });

  it("are never named by a third-party address in the source", () => {
    const font = /https?:\/\/[^\s"'`)]*(?:fonts\.(?:googleapis|gstatic)\.com|\.(?:woff2?|ttf|otf)\b)[^\s"'`)]*/g;
    const offenders = SOURCE.flatMap((file) =>
      (readFileSync(file, "utf8").match(font) ?? []).map((hit) => `${relative(FRONT, file)}: ${hit}`),
    );
    expect(offenders).toEqual([]);
  });

  it("are served by the binary, with their licence", () => {
    const fonts = join(FRONT, "public", "shared", "fonts");
    for (const [, path] of TOKENS.matchAll(/url\("\/shared\/fonts\/([^"]+)"\)/g)) {
      expect(existsSync(join(fonts, path)), path).toBe(true);
    }
    expect(readFileSync(join(fonts, "Archivo-OFL.txt"), "utf8")).toContain("SIL Open Font License, Version 1.1");
    expect(readFileSync(join(fonts, "GeistMono-OFL.txt"), "utf8")).toContain("SIL Open Font License, Version 1.1");
  });
});
