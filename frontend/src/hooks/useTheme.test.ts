import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { act, renderHook } from "@testing-library/react";
import { initTheme, useTheme } from "./useTheme";

function stubOs(dark: boolean) {
  vi.stubGlobal("matchMedia", (query: string) => ({ media: query, matches: query.includes("dark") === dark }));
}

const attributes = () => [
  document.documentElement.getAttribute("data-theme"),
  document.documentElement.getAttribute("data-mode"),
];

beforeEach(() => {
  localStorage.clear();
  document.documentElement.removeAttribute("data-theme");
  document.documentElement.removeAttribute("data-mode");
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("useTheme (ADR-0093)", () => {
  it("starts on Acogni and dark when nothing is stored, whatever the OS asks", () => {
    stubOs(false);
    initTheme();
    const { result } = renderHook(() => useTheme());
    expect([result.current.theme, result.current.mode]).toEqual(["acogni", "dark"]);
    expect(attributes()).toEqual(["acogni", "dark"]);
  });

  it("starts on the stored choice", () => {
    localStorage.setItem("pdo.ui.theme", "classic");
    localStorage.setItem("pdo.ui.mode", "light");
    initTheme();
    const { result } = renderHook(() => useTheme());
    expect([result.current.theme, result.current.mode]).toEqual(["classic", "light"]);
    expect(attributes()).toEqual(["classic", "light"]);
  });

  it("migrates a System preference to the OS's mode at boot", () => {
    localStorage.setItem("pdo.ui.theme", "system");
    stubOs(false);
    initTheme();
    const { result } = renderHook(() => useTheme());
    expect([result.current.theme, result.current.mode]).toEqual(["acogni", "light"]);
  });

  it("repaints and persists the Theme and the mode, each on its own", () => {
    initTheme();
    const { result } = renderHook(() => useTheme());
    act(() => result.current.setTheme("classic"));
    expect(attributes()).toEqual(["classic", "dark"]);
    act(() => result.current.toggleMode());
    expect(attributes()).toEqual(["classic", "light"]);
    expect([result.current.theme, result.current.mode]).toEqual(["classic", "light"]);
    expect([localStorage.getItem("pdo.ui.theme"), localStorage.getItem("pdo.ui.mode")]).toEqual(["classic", "light"]);
    act(() => result.current.setMode("dark"));
    act(() => result.current.setTheme("acogni"));
    expect(attributes()).toEqual(["acogni", "dark"]);
  });

  it("keeps two mounted consumers in sync", () => {
    initTheme();
    const a = renderHook(() => useTheme());
    const b = renderHook(() => useTheme());
    act(() => a.result.current.setTheme("classic"));
    expect(b.result.current.theme).toBe("classic");
  });

  it("follows a change another tab wrote", () => {
    initTheme();
    const { result } = renderHook(() => useTheme());
    localStorage.setItem("pdo.ui.theme", "classic");
    localStorage.setItem("pdo.ui.mode", "light");
    act(() => {
      window.dispatchEvent(new StorageEvent("storage", { key: "pdo.ui.mode" }));
    });
    expect([result.current.theme, result.current.mode]).toEqual(["classic", "light"]);
    expect(attributes()).toEqual(["classic", "light"]);
  });
});
