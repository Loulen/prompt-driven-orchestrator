/**
 * Themeability guard (#759).
 *
 * A light theme only stays light if components keep painting through the
 * semantic tokens. One `text-[#04140d]` is invisible in review and silently
 * paints a dark-theme colour onto a light page — so the rule is enforced here
 * rather than trusted.
 *
 * Scope is deliberately narrow: Tailwind arbitrary-value COLOUR classes. A hex
 * elsewhere is legitimate (a `var()` fallback, the terminal's ANSI palette, a
 * `cssColor` default), and banning those would be noise.
 */
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { describe, it, expect } from "vitest";

const SRC = import.meta.dirname;

function sourceFiles(dir: string): string[] {
  return readdirSync(dir).flatMap((entry) => {
    const path = join(dir, entry);
    if (statSync(path).isDirectory()) return sourceFiles(path);
    return /\.tsx?$/.test(path) && !path.includes(".test.") ? [path] : [];
  });
}

/** e.g. `text-[#04140d]`, `hover:bg-[#14cf92]`, `dark:border-[#fff]`. */
const ARBITRARY_COLOUR = /\b[a-z-]*(?:text|bg|border|fill|stroke|ring|from|to|via|decoration|outline|shadow|accent|caret|divide)-\[#[0-9a-fA-F]{3,8}\]/g;

describe("themeability (#759)", () => {
  it("paints through palette tokens, never a Tailwind arbitrary hex", () => {
    const offenders = sourceFiles(SRC).flatMap((file) => {
      const hits = readFileSync(file, "utf8").match(ARBITRARY_COLOUR) ?? [];
      return hits.map((hit) => `${file.slice(SRC.length + 1)}: ${hit}`);
    });
    expect(offenders).toEqual([]);
  });
});
