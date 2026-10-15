import { afterEach, describe, expect, it, vi } from "vitest";
import {
  applyColourBlindStatus,
  COLOUR_BLIND_STATUS_STORAGE_KEY,
  loadColourBlindStatus,
  saveColourBlindStatus,
} from "./statusPalette";

afterEach(() => {
  localStorage.clear();
  vi.restoreAllMocks();
  document.documentElement.removeAttribute("data-status-palette");
});

describe("colour-blind status preference (#1005)", () => {
  it("is off by default", () => {
    expect(loadColourBlindStatus()).toBe(false);
  });

  it("round-trips through the pdo.ui namespace", () => {
    saveColourBlindStatus(true);
    expect(localStorage.getItem(COLOUR_BLIND_STATUS_STORAGE_KEY)).toBe("true");
    expect(COLOUR_BLIND_STATUS_STORAGE_KEY.startsWith("pdo.ui.")).toBe(true);
    expect(loadColourBlindStatus()).toBe(true);
    saveColourBlindStatus(false);
    expect(loadColourBlindStatus()).toBe(false);
  });

  it("reads an unreadable value as off", () => {
    for (const junk of ["1", "yes", "TRUE", "{}", ""]) {
      localStorage.setItem(COLOUR_BLIND_STATUS_STORAGE_KEY, junk);
      expect(loadColourBlindStatus()).toBe(false);
    }
  });

  it("falls back to off, without throwing, when the store is unavailable", () => {
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("SecurityError");
    });
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("QuotaExceededError");
    });
    expect(() => saveColourBlindStatus(true)).not.toThrow();
    expect(loadColourBlindStatus()).toBe(false);
  });

  it("stamps the attribute on <html> only while on", () => {
    const root = document.documentElement;
    applyColourBlindStatus(true);
    expect(root.getAttribute("data-status-palette")).toBe("colour-blind");
    applyColourBlindStatus(false);
    expect(root.hasAttribute("data-status-palette")).toBe(false);
  });
});
