import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { act, renderHook } from "@testing-library/react";
import { initTheme, useTheme } from "./useTheme";

/** A `prefers-color-scheme` stub whose answer can be flipped, as the OS would. */
function stubOs(dark: boolean) {
  const listeners = new Set<(e: { matches: boolean }) => void>();
  let current = dark;
  vi.stubGlobal("matchMedia", (query: string) => ({
    media: query,
    get matches() {
      return current;
    },
    addEventListener: (_: string, cb: (e: { matches: boolean }) => void) => listeners.add(cb),
    removeEventListener: (_: string, cb: (e: { matches: boolean }) => void) => listeners.delete(cb),
  }));
  return {
    flip(next: boolean) {
      current = next;
      for (const cb of listeners) cb({ matches: next });
    },
  };
}

beforeEach(() => {
  localStorage.clear();
  document.documentElement.removeAttribute("data-theme");
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("useTheme (#759)", () => {
  it("starts dark when nothing is stored, whatever the OS asks (#781)", () => {
    stubOs(false);
    initTheme();
    const { result } = renderHook(() => useTheme());
    expect(result.current.preference).toBe("dark");
    expect(result.current.resolved).toBe("dark");
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
  });

  it("starts on the stored choice, whatever the OS says", () => {
    localStorage.setItem("pdo.ui.theme", "light");
    stubOs(true);
    initTheme();
    const { result } = renderHook(() => useTheme());
    expect(result.current.resolved).toBe("light");
    expect(document.documentElement.getAttribute("data-theme")).toBe("light");
  });

  it("repaints and persists when the preference changes", () => {
    stubOs(true);
    initTheme();
    const { result } = renderHook(() => useTheme());
    act(() => result.current.setPreference("light"));
    expect(result.current.resolved).toBe("light");
    expect(document.documentElement.getAttribute("data-theme")).toBe("light");
    expect(localStorage.getItem("pdo.ui.theme")).toBe("light");
  });

  it("follows the OS while the preference is system", () => {
    const os = stubOs(true);
    initTheme();
    const { result } = renderHook(() => useTheme());
    // #781: nothing stored now pins `dark` — following the OS requires an
    // explicit `system` choice.
    act(() => result.current.setPreference("system"));
    expect(result.current.resolved).toBe("dark");
    act(() => os.flip(false));
    expect(result.current.resolved).toBe("light");
    expect(document.documentElement.getAttribute("data-theme")).toBe("light");
  });

  it("ignores the OS once a theme is pinned", () => {
    const os = stubOs(true);
    initTheme();
    const { result } = renderHook(() => useTheme());
    act(() => result.current.setPreference("dark"));
    act(() => os.flip(false));
    expect(result.current.resolved).toBe("dark");
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
  });

  it("keeps two mounted consumers in sync", () => {
    stubOs(true);
    initTheme();
    const a = renderHook(() => useTheme());
    const b = renderHook(() => useTheme());
    act(() => a.result.current.setPreference("light"));
    expect(b.result.current.resolved).toBe("light");
  });
});
