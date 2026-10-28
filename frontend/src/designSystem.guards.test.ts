/**
 * S4 — the guards of the shared design system (ADR-0091).
 *
 *  - The palette and the roles are defined in both themes.
 *  - The old token names are gone, everywhere: no alias.
 *  - No font size in pixels and no hex colour in the source, except in the
 *    files `designSystem.unmigrated.ts` lists; that list only shrinks.
 *  - No font is fetched from a third party: the binary serves them.
 */
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import { describe, expect, it } from "vitest";
import { UNMIGRATED } from "./designSystem.unmigrated";

const SRC = import.meta.dirname;
const FRONT = join(SRC, "..");
const TOKENS = readFileSync(join(FRONT, "public", "shared", "tokens.css"), "utf8");
const INDEX_CSS = readFileSync(join(SRC, "index.css"), "utf8");

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
  ...walk(join(FRONT, "..", "crates", "pdo", "tests", "test_extension"), (p) => p.endsWith(".js")),
];

/** The custom properties a CSS block declares. */
function declared(opener: string): Set<string> {
  const start = TOKENS.indexOf(opener);
  if (start < 0) throw new Error(`block not found: ${opener}`);
  const end = TOKENS.indexOf("}", start);
  return new Set([...TOKENS.slice(start, end).matchAll(/(--[\w-]+)\s*:/g)].map((m) => m[1]));
}

describe("tokens", () => {
  it("define the same palette in the dark and the light theme", () => {
    const dark = declared(':root,\n[data-theme="dark"] {');
    const light = declared('[data-theme="light"] {');
    expect([...light].sort()).toEqual([...dark].sort());
  });

  it("define every role Tailwind's utilities paint with", () => {
    const roles = declared(":root,\n[data-theme] {");
    const palette = declared(':root,\n[data-theme="dark"] {');
    const theme = INDEX_CSS.slice(INDEX_CSS.indexOf("@theme inline reference {"));
    const used = [...theme.slice(0, theme.indexOf("\n}")).matchAll(/--color-[\w-]+:\s*var\((--[\w-]+)\)/g)].map((m) => m[1]);
    expect(used.length).toBeGreaterThan(20);
    expect(used.filter((name) => !roles.has(name) && !palette.has(name))).toEqual([]);
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
  const offending = SOURCE.filter((file) => unmigrated(readFileSync(file, "utf8"))).map((file) => relative(SRC, file));

  it("hold everywhere but in the files not migrated yet", () => {
    expect(offending.filter((file) => !UNMIGRATED.includes(file))).toEqual([]);
  });

  it("list only files still to migrate: a migrated file leaves the list", () => {
    expect(UNMIGRATED.filter((file) => !existsSync(join(SRC, file)) || !offending.includes(file))).toEqual([]);
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
