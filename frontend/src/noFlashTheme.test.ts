/**
 * The anti-flash guards (#759, ADR-0093). `index.html` loads the shared theme
 * script (`public/shared/theme.js`) and an inline script for the UI scale,
 * both BEFORE the bundle, so the page never paints one look and then flips.
 * The theme script duplicates the rule `lib/theme.ts` owns, migration
 * included: this test runs the real script and asserts it agrees with the
 * library on every case.
 */
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, it, expect, beforeEach } from "vitest";
import { loadAppearance } from "./lib/theme";
import { loadUiScale } from "./lib/uiScale";

const FRONT = join(import.meta.dirname, "..");
const HTML = readFileSync(join(FRONT, "index.html"), "utf8");
const THEME_SCRIPT = readFileSync(join(FRONT, "public", "shared", "theme.js"), "utf8");

function inlineScaleGuard(): string {
  const match = HTML.match(/<script>([\s\S]*?)<\/script>/);
  if (!match) throw new Error("no inline script in index.html");
  return match[1];
}

/** A store of the given values, with what was written to it. */
function store(values: Record<string, string | null>) {
  const data = { ...values };
  return {
    data,
    getItem: (key: string) => data[key] ?? null,
    setItem: (key: string, value: string) => (data[key] = value),
  };
}

/** Run `script` against a stubbed document, store and OS; return what it stamped. */
function run(script: string, localStorage: unknown, osDark = true) {
  const root: Record<string, unknown> = {};
  const listeners: Record<string, (event: { key: string | null }) => void> = {};
  const fake = {
    documentElement: {
      setAttribute: (name: string, value: string) => (root[name] = value),
      style: {
        setProperty: (name: string, value: string) => (root[name] = value),
        set colorScheme(value: string) {
          root["color-scheme"] = value;
        },
      },
    },
  };
  const window = { addEventListener: (type: string, cb: (event: { key: string | null }) => void) => (listeners[type] = cb) };
  const matchMedia = (query: string) => ({ matches: query.includes("dark") === osDark });
  new Function("document", "localStorage", "matchMedia", "window", script)(fake, localStorage, matchMedia, window);
  return { root, listeners };
}

beforeEach(() => window.localStorage.clear());

describe("the theme script", () => {
  it("is loaded by index.html, from its stable address, before the app bundle", () => {
    const tag = HTML.indexOf('<script src="/shared/theme.js"></script>');
    expect(tag).toBeGreaterThan(0);
    expect(tag).toBeLessThan(HTML.indexOf("/src/main.tsx"));
  });

  it("agrees with lib/theme on every stored value and OS, migration included", () => {
    const themes = ["acogni", "classic", "light", "dark", "system", "nonsense", null];
    const modes = ["dark", "light", "dim", null];
    for (const theme of themes) {
      for (const mode of modes) {
        for (const osDark of [true, false]) {
          const values = { "pdo.ui.theme": theme, "pdo.ui.mode": mode };
          const stub = store(values);
          const { root } = run(THEME_SCRIPT, stub, osDark);

          window.localStorage.clear();
          for (const [key, value] of Object.entries(values)) if (value !== null) window.localStorage.setItem(key, value);
          const expected = loadAppearance(osDark);
          expect({ theme, mode, osDark, got: [root["data-theme"], root["data-mode"], root["color-scheme"]] }).toEqual({
            theme,
            mode,
            osDark,
            got: [expected.theme, expected.mode, expected.mode],
          });
          // What it leaves in the store is what the library leaves.
          expect([stub.data["pdo.ui.theme"], stub.data["pdo.ui.mode"]]).toEqual([
            window.localStorage.getItem("pdo.ui.theme"),
            window.localStorage.getItem("pdo.ui.mode"),
          ]);
        }
      }
    }
  });

  it("repaints when another page changes the preferences", () => {
    const stub = store({ "pdo.ui.theme": "acogni", "pdo.ui.mode": "dark" });
    const { root, listeners } = run(THEME_SCRIPT, stub);
    expect(root["data-theme"]).toBe("acogni");
    stub.setItem("pdo.ui.theme", "classic");
    stub.setItem("pdo.ui.mode", "light");
    listeners.storage({ key: "pdo.ui.theme" });
    expect([root["data-theme"], root["data-mode"]]).toEqual(["classic", "light"]);
  });

  it("survives a store that throws, rather than blanking the page", () => {
    const throwing = {
      getItem() {
        throw new Error("SecurityError");
      },
      setItem() {
        throw new Error("SecurityError");
      },
    };
    const { root } = run(THEME_SCRIPT, throwing);
    expect([root["data-theme"], root["data-mode"]]).toEqual(["acogni", "dark"]);
  });
});

describe("index.html UI scale guard", () => {
  it("runs before the app bundle", () => {
    expect(HTML.indexOf("<script>")).toBeLessThan(HTML.indexOf("/src/main.tsx"));
  });

  it("stamps the UI scale lib/uiScale would load", () => {
    for (const scale of ["100", "110", "120", null, "90", "nonsense"]) {
      window.localStorage.clear();
      if (scale !== null) window.localStorage.setItem("pdo.ui.scale", scale);
      const { root } = run(inlineScaleGuard(), store({ "pdo.ui.scale": scale }));
      expect({ scale, zoom: root["--app-zoom"] }).toEqual({ scale, zoom: String(loadUiScale() / 100) });
    }
  });

  it("survives a store that throws", () => {
    const throwing = {
      getItem() {
        throw new Error("SecurityError");
      },
    };
    expect(() => run(inlineScaleGuard(), throwing)).not.toThrow();
  });
});
