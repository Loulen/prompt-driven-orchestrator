/**
 * The anti-FOUC guard (#759). `index.html` carries a tiny inline script that
 * stamps the theme on `<html>` BEFORE the bundle loads, so the page never
 * paints dark and then flips. It necessarily duplicates the resolution rule
 * that `lib/theme.ts` owns, so this test runs the real script — extracted from
 * the shipped HTML — and asserts it agrees with the library on every case.
 */
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, it, expect } from "vitest";
import { resolveTheme, type ThemePreference } from "./lib/theme";

const HTML = readFileSync(join(import.meta.dirname, "..", "index.html"), "utf8");

function inlineGuard(): string {
  const match = HTML.match(/<script>([\s\S]*?)<\/script>/);
  if (!match) throw new Error("no inline script in index.html");
  return match[1];
}

/** Run the guard against a stubbed store and OS, return what it stamped. */
function runGuard(stored: string | null, osDark: boolean): string | null {
  const root: Record<string, unknown> = {};
  const fake = {
    documentElement: {
      setAttribute: (name: string, value: string) => (root[name] = value),
      style: {} as Record<string, string>,
    },
  };
  const localStorage = { getItem: () => stored };
  const matchMedia = (query: string) => ({ matches: query.includes("dark") === osDark });
  new Function("document", "localStorage", "matchMedia", inlineGuard())(
    fake,
    localStorage,
    matchMedia,
  );
  return (root["data-theme"] as string) ?? null;
}

describe("index.html theme guard (#759)", () => {
  it("runs before the app bundle, so the first paint is already themed", () => {
    expect(HTML.indexOf("<script>")).toBeLessThan(HTML.indexOf("/src/main.tsx"));
  });

  it("agrees with lib/theme on every stored value and OS combination", () => {
    for (const stored of ["light", "dark", "system", null, "nonsense"]) {
      for (const osDark of [true, false]) {
        const preference = (
          ["light", "dark", "system"].includes(stored ?? "") ? stored : "dark"
        ) as ThemePreference;
        expect({ stored, osDark, got: runGuard(stored, osDark) }).toEqual({
          stored,
          osDark,
          got: resolveTheme(preference, osDark),
        });
      }
    }
  });

  it("survives a store that throws, rather than blanking the page", () => {
    const guard = inlineGuard();
    const throwing = {
      getItem() {
        throw new Error("SecurityError");
      },
    };
    expect(() =>
      new Function("document", "localStorage", "matchMedia", guard)(
        { documentElement: { setAttribute: () => {}, style: {} } },
        throwing,
        () => ({ matches: true }),
      ),
    ).not.toThrow();
  });
});
