/**
 * The anti-flash guard, colour-blind status palette half (#1005). The inline
 * script of `index.html` stamps `data-status-palette` on `<html>` BEFORE the
 * bundle loads, so an opted-in browser never paints the default status colours
 * and then flips. It duplicates the rule `lib/statusPalette.ts` owns, so this
 * test runs the real script — extracted from the shipped HTML — and asserts the
 * two agree on every stored value.
 */
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { afterEach, describe, it, expect } from "vitest";
import {
  COLOUR_BLIND_STATUS_STORAGE_KEY,
  loadColourBlindStatus,
  STATUS_PALETTE_ATTRIBUTE,
} from "./lib/statusPalette";

const HTML = readFileSync(join(import.meta.dirname, "..", "index.html"), "utf8");

function inlineGuard(): string {
  const match = HTML.match(/<script>([\s\S]*?)<\/script>/);
  if (!match) throw new Error("no inline script in index.html");
  return match[1];
}

/** Run the guard against a stubbed store, return the attributes it stamped. */
function runGuard(store: Record<string, string | null>): Record<string, string> {
  const root: Record<string, string> = {};
  const fake = {
    documentElement: {
      setAttribute: (name: string, value: string) => (root[name] = value),
      style: {} as Record<string, string>,
    },
  };
  const localStorage = { getItem: (key: string) => store[key] ?? null };
  const matchMedia = () => ({ matches: true });
  new Function("document", "localStorage", "matchMedia", inlineGuard())(
    fake,
    localStorage,
    matchMedia,
  );
  return root;
}

afterEach(() => localStorage.clear());

describe("index.html colour-blind status palette guard (#1005)", () => {
  it("runs before the app bundle, so the first paint already has the chosen palette", () => {
    expect(HTML.indexOf("<script>")).toBeLessThan(HTML.indexOf("/src/main.tsx"));
    expect(inlineGuard()).toContain(COLOUR_BLIND_STATUS_STORAGE_KEY);
  });

  it("agrees with lib/statusPalette on every stored value", () => {
    for (const stored of ["true", "false", null, "1", "TRUE", "nonsense"]) {
      if (stored === null) localStorage.removeItem(COLOUR_BLIND_STATUS_STORAGE_KEY);
      else localStorage.setItem(COLOUR_BLIND_STATUS_STORAGE_KEY, stored);
      const stamped = runGuard({ [COLOUR_BLIND_STATUS_STORAGE_KEY]: stored });
      expect({ stored, on: STATUS_PALETTE_ATTRIBUTE in stamped }).toEqual({
        stored,
        on: loadColourBlindStatus(),
      });
      if (STATUS_PALETTE_ATTRIBUTE in stamped) {
        expect(stamped[STATUS_PALETTE_ATTRIBUTE]).toBe("colour-blind");
      }
    }
  });

  it("stamps the palette whatever the theme, next to data-theme", () => {
    for (const theme of ["light", "dark"]) {
      const stamped = runGuard({ "pdo.ui.theme": theme, [COLOUR_BLIND_STATUS_STORAGE_KEY]: "true" });
      expect(stamped["data-theme"]).toBe(theme);
      expect(stamped[STATUS_PALETTE_ATTRIBUTE]).toBe("colour-blind");
    }
  });

  it("leaves the default palette, without throwing, when the store throws", () => {
    const root: Record<string, string> = {};
    const throwing = {
      getItem() {
        throw new Error("SecurityError");
      },
    };
    expect(() =>
      new Function("document", "localStorage", "matchMedia", inlineGuard())(
        { documentElement: { setAttribute: (n: string, v: string) => (root[n] = v), style: {} } },
        throwing,
        () => ({ matches: true }),
      ),
    ).not.toThrow();
    expect(STATUS_PALETTE_ATTRIBUTE in root).toBe(false);
  });
});
