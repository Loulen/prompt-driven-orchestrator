import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import {
  loadThemePreference,
  saveThemePreference,
  resolveTheme,
  systemPrefersDark,
  applyResolvedTheme,
  currentTheme,
} from "./theme";

beforeEach(() => {
  localStorage.clear();
});

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("theme preference — persistence (#759)", () => {
  it("round-trips each of the three preferences", () => {
    for (const pref of ["light", "dark", "system"] as const) {
      saveThemePreference(pref);
      expect(loadThemePreference()).toBe(pref);
    }
  });

  it("persists under the pdo.ui.theme key", () => {
    saveThemePreference("light");
    expect(localStorage.getItem("pdo.ui.theme")).toBe("light");
  });

  it("defaults to system when nothing is stored", () => {
    expect(loadThemePreference()).toBe("system");
  });

  it("defaults to system for an unknown stored value", () => {
    localStorage.setItem("pdo.ui.theme", "solarized");
    expect(loadThemePreference()).toBe("system");
  });

  it("degrades to system when getItem throws (private mode)", () => {
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("SecurityError");
    });
    expect(loadThemePreference()).toBe("system");
  });

  it("swallows a throwing setItem (quota / disabled) without raising", () => {
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("QuotaExceededError");
    });
    expect(() => saveThemePreference("dark")).not.toThrow();
  });
});

describe("theme resolution — system defers to the OS (#759)", () => {
  it("pins light and dark whatever the OS says", () => {
    expect(resolveTheme("light", true)).toBe("light");
    expect(resolveTheme("light", false)).toBe("light");
    expect(resolveTheme("dark", true)).toBe("dark");
    expect(resolveTheme("dark", false)).toBe("dark");
  });

  it("follows the OS when the preference is system", () => {
    expect(resolveTheme("system", true)).toBe("dark");
    expect(resolveTheme("system", false)).toBe("light");
  });

  it("reads prefers-color-scheme: dark for the OS answer", () => {
    vi.stubGlobal(
      "matchMedia",
      vi.fn((q: string) => ({ matches: q.includes("dark"), media: q })),
    );
    expect(systemPrefersDark()).toBe(true);
    vi.stubGlobal("matchMedia", vi.fn(() => ({ matches: false })));
    expect(systemPrefersDark()).toBe(false);
  });

  it("stays dark when matchMedia is unavailable (preserves the existing look)", () => {
    vi.stubGlobal("matchMedia", undefined);
    expect(systemPrefersDark()).toBe(true);
  });
});

describe("theme application — the root element carries the resolved theme (#759)", () => {
  it("stamps data-theme with the resolved value, never with 'system'", () => {
    const root = document.documentElement;
    applyResolvedTheme("light", root);
    expect(root.getAttribute("data-theme")).toBe("light");
    applyResolvedTheme("dark", root);
    expect(root.getAttribute("data-theme")).toBe("dark");
  });

  it("sets color-scheme so native widgets and scrollbars follow", () => {
    const root = document.documentElement;
    applyResolvedTheme("light", root);
    expect(root.style.colorScheme).toBe("light");
    applyResolvedTheme("dark", root);
    expect(root.style.colorScheme).toBe("dark");
  });
});

describe("currentTheme — what is painted right now (#759)", () => {
  it("reads back what applyResolvedTheme stamped", () => {
    applyResolvedTheme("light");
    expect(currentTheme()).toBe("light");
    applyResolvedTheme("dark");
    expect(currentTheme()).toBe("dark");
  });

  it("answers dark when nothing has been stamped, matching the historical look", () => {
    document.documentElement.removeAttribute("data-theme");
    expect(currentTheme()).toBe("dark");
  });

  it("answers dark for a value it does not recognise", () => {
    document.documentElement.setAttribute("data-theme", "sepia");
    expect(currentTheme()).toBe("dark");
  });
});
