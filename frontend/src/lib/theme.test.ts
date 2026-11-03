import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import {
  applyThemeChoice,
  currentMode,
  loadThemeChoice,
  saveMode,
  saveTheme,
  systemPrefersDark,
} from "./theme";

beforeEach(() => {
  localStorage.clear();
});

afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe("Theme and Dark mode: persistence (ADR-0093)", () => {
  it("default to Acogni and dark when nothing is stored", () => {
    expect(loadThemeChoice(false)).toEqual({ theme: "acogni", mode: "dark" });
  });

  it("round-trip each Theme and each mode, independently", () => {
    for (const theme of ["acogni", "classic"] as const) {
      for (const mode of ["dark", "light"] as const) {
        saveTheme(theme);
        saveMode(mode);
        expect(loadThemeChoice()).toEqual({ theme, mode });
      }
    }
    expect(localStorage.getItem("pdo.ui.theme")).toBe("classic");
    expect(localStorage.getItem("pdo.ui.mode")).toBe("light");
  });

  it("fall back to the default for an unknown stored value, each on its own", () => {
    localStorage.setItem("pdo.ui.theme", "solarized");
    localStorage.setItem("pdo.ui.mode", "light");
    expect(loadThemeChoice()).toEqual({ theme: "acogni", mode: "light" });
    localStorage.setItem("pdo.ui.theme", "classic");
    localStorage.setItem("pdo.ui.mode", "dim");
    expect(loadThemeChoice()).toEqual({ theme: "classic", mode: "dark" });
  });

  it("rewrite an unknown stored value as the default, as a migrated one is", () => {
    localStorage.setItem("pdo.ui.theme", "garbage");
    localStorage.setItem("pdo.ui.mode", "light");
    expect(loadThemeChoice()).toEqual({ theme: "acogni", mode: "light" });
    expect([localStorage.getItem("pdo.ui.theme"), localStorage.getItem("pdo.ui.mode")]).toEqual(["acogni", "light"]);
    localStorage.setItem("pdo.ui.mode", "dim");
    loadThemeChoice();
    expect(localStorage.getItem("pdo.ui.mode")).toBe("dark");
  });

  it("write nothing when nothing is stored", () => {
    loadThemeChoice();
    expect([localStorage.getItem("pdo.ui.theme"), localStorage.getItem("pdo.ui.mode")]).toEqual([null, null]);
  });

  it("degrade to the defaults when the store throws (private mode)", () => {
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("SecurityError");
    });
    expect(loadThemeChoice()).toEqual({ theme: "acogni", mode: "dark" });
  });

  it("swallow a throwing setItem (quota / disabled) without raising", () => {
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("QuotaExceededError");
    });
    expect(() => saveTheme("classic")).not.toThrow();
    expect(() => saveMode("light")).not.toThrow();
  });
});

describe("migration of the preference stored before 2.68", () => {
  it("keeps light and dark as the mode, under the Acogni Theme", () => {
    for (const mode of ["light", "dark"] as const) {
      for (const osDark of [true, false]) {
        localStorage.clear();
        localStorage.setItem("pdo.ui.theme", mode);
        expect(loadThemeChoice(osDark)).toEqual({ theme: "acogni", mode });
      }
    }
  });

  it("turns System into the mode the OS asks for at the migration", () => {
    localStorage.setItem("pdo.ui.theme", "system");
    expect(loadThemeChoice(false)).toEqual({ theme: "acogni", mode: "light" });
    localStorage.setItem("pdo.ui.theme", "system");
    expect(loadThemeChoice(true)).toEqual({ theme: "acogni", mode: "dark" });
  });

  it("stores the result, so a later change of the OS no longer moves it", () => {
    localStorage.setItem("pdo.ui.theme", "system");
    loadThemeChoice(false);
    expect(localStorage.getItem("pdo.ui.theme")).toBe("acogni");
    expect(localStorage.getItem("pdo.ui.mode")).toBe("light");
    expect(loadThemeChoice(true)).toEqual({ theme: "acogni", mode: "light" });
  });

  it("migrates even when the store refuses the write, for this session", () => {
    localStorage.setItem("pdo.ui.theme", "light");
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("QuotaExceededError");
    });
    expect(loadThemeChoice()).toEqual({ theme: "acogni", mode: "light" });
  });
});

describe("the OS answer", () => {
  it("reads prefers-color-scheme: dark", () => {
    vi.stubGlobal(
      "matchMedia",
      vi.fn((q: string) => ({ matches: q.includes("dark"), media: q })),
    );
    expect(systemPrefersDark()).toBe(true);
    vi.stubGlobal("matchMedia", vi.fn(() => ({ matches: false })));
    expect(systemPrefersDark()).toBe(false);
  });

  it("is dark when matchMedia is unavailable", () => {
    vi.stubGlobal("matchMedia", undefined);
    expect(systemPrefersDark()).toBe(true);
  });
});

describe("the attributes of the document", () => {
  it("carry the Theme on data-theme and the mode on data-mode, with color-scheme", () => {
    const root = document.documentElement;
    applyThemeChoice({ theme: "classic", mode: "light" }, root);
    expect(root.getAttribute("data-theme")).toBe("classic");
    expect(root.getAttribute("data-mode")).toBe("light");
    expect(root.style.colorScheme).toBe("light");
    applyThemeChoice({ theme: "acogni", mode: "dark" }, root);
    expect(root.getAttribute("data-theme")).toBe("acogni");
    expect(root.getAttribute("data-mode")).toBe("dark");
    expect(root.style.colorScheme).toBe("dark");
  });

  it("read back the mode painted, dark when none or an unknown one", () => {
    applyThemeChoice({ theme: "classic", mode: "light" });
    expect(currentMode()).toBe("light");
    document.documentElement.removeAttribute("data-mode");
    expect(currentMode()).toBe("dark");
    document.documentElement.setAttribute("data-mode", "sepia");
    expect(currentMode()).toBe("dark");
  });
});
