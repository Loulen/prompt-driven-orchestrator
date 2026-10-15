import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { act, renderHook } from "@testing-library/react";
import {
  initColourBlindStatus,
  setColourBlindStatus,
  useColourBlindStatus,
} from "./useColourBlindStatus";
import { COLOUR_BLIND_STATUS_STORAGE_KEY } from "../lib/statusPalette";

const root = document.documentElement;

/** What another tab does: write the store, then this tab gets a `storage` event. */
function otherTabWrites(value: string | null) {
  if (value === null) localStorage.removeItem(COLOUR_BLIND_STATUS_STORAGE_KEY);
  else localStorage.setItem(COLOUR_BLIND_STATUS_STORAGE_KEY, value);
  window.dispatchEvent(
    new StorageEvent("storage", { key: COLOUR_BLIND_STATUS_STORAGE_KEY, newValue: value }),
  );
}

beforeEach(() => {
  localStorage.clear();
  root.removeAttribute("data-status-palette");
  initColourBlindStatus();
});

afterEach(() => {
  act(() => setColourBlindStatus(false));
  localStorage.clear();
});

describe("useColourBlindStatus (#1005)", () => {
  it("starts off, with no attribute on <html>", () => {
    const { result } = renderHook(() => useColourBlindStatus());
    expect(result.current.enabled).toBe(false);
    expect(root.hasAttribute("data-status-palette")).toBe(false);
  });

  it("applies at the switch and stores it, without any Save", () => {
    const { result } = renderHook(() => useColourBlindStatus());
    act(() => result.current.setEnabled(true));
    expect(result.current.enabled).toBe(true);
    expect(root.getAttribute("data-status-palette")).toBe("colour-blind");
    expect(localStorage.getItem(COLOUR_BLIND_STATUS_STORAGE_KEY)).toBe("true");
    act(() => result.current.setEnabled(false));
    expect(root.hasAttribute("data-status-palette")).toBe(false);
  });

  it("adopts the stored value at boot", () => {
    localStorage.setItem(COLOUR_BLIND_STATUS_STORAGE_KEY, "true");
    act(() => {
      initColourBlindStatus();
    });
    const { result } = renderHook(() => useColourBlindStatus());
    expect(result.current.enabled).toBe(true);
    expect(root.getAttribute("data-status-palette")).toBe("colour-blind");
  });

  it("follows a switch made in another tab, both ways", () => {
    const { result } = renderHook(() => useColourBlindStatus());
    act(() => otherTabWrites("true"));
    expect(result.current.enabled).toBe(true);
    expect(root.getAttribute("data-status-palette")).toBe("colour-blind");
    act(() => otherTabWrites("false"));
    expect(result.current.enabled).toBe(false);
    expect(root.hasAttribute("data-status-palette")).toBe(false);
  });

  it("ignores storage events for other keys", () => {
    const { result } = renderHook(() => useColourBlindStatus());
    act(() => {
      localStorage.setItem(COLOUR_BLIND_STATUS_STORAGE_KEY, "true");
      window.dispatchEvent(new StorageEvent("storage", { key: "pdo.ui.theme", newValue: "light" }));
    });
    expect(result.current.enabled).toBe(false);
  });
});
