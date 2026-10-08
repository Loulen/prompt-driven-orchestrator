import { describe, expect, it, vi } from "vitest";
import { copyToClipboard } from "./clipboard";

describe("copyToClipboard (#772)", () => {
  it("prefers navigator.clipboard when present", async () => {
    const writeText = vi.fn(async () => undefined);
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    const execCommand = vi.fn(() => true);
    Object.defineProperty(document, "execCommand", { value: execCommand, configurable: true });
    expect(await copyToClipboard("abc")).toBe(true);
    expect(writeText).toHaveBeenCalledWith("abc");
    expect(execCommand).not.toHaveBeenCalled();
  });

  it("falls back to execCommand('copy') on an insecure context (no navigator.clipboard)", async () => {
    Object.defineProperty(navigator, "clipboard", { value: undefined, configurable: true });
    const execCommand = vi.fn(() => {
      // The text to copy must be selected in the document at that moment.
      const ta = document.activeElement as HTMLTextAreaElement;
      expect(ta.tagName).toBe("TEXTAREA");
      expect(ta.value).toBe("from-vps");
      return true;
    });
    Object.defineProperty(document, "execCommand", { value: execCommand, configurable: true });
    expect(await copyToClipboard("from-vps")).toBe(true);
    expect(execCommand).toHaveBeenCalledWith("copy");
    // The throwaway textarea is gone again.
    expect(document.querySelector("textarea")).toBeNull();
  });

  it("falls back to execCommand when the Clipboard API rejects", async () => {
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText: vi.fn(async () => { throw new Error("denied"); }) },
      configurable: true,
    });
    const execCommand = vi.fn(() => true);
    Object.defineProperty(document, "execCommand", { value: execCommand, configurable: true });
    expect(await copyToClipboard("x")).toBe(true);
    expect(execCommand).toHaveBeenCalledWith("copy");
  });

  it("reports failure when neither path works", async () => {
    Object.defineProperty(navigator, "clipboard", { value: undefined, configurable: true });
    Object.defineProperty(document, "execCommand", { value: vi.fn(() => false), configurable: true });
    expect(await copyToClipboard("x")).toBe(false);
  });
});

describe("every Copy of the core", () => {
  it("goes through copyToClipboard: no other module writes to navigator.clipboard", () => {
    const sources = import.meta.glob(["../../**/*.{ts,tsx}", "!../../**/*.test.{ts,tsx}"], {
      query: "?raw",
      import: "default",
      eager: true,
    }) as Record<string, string>;
    expect(Object.keys(sources).length).toBeGreaterThan(100);
    const writers = Object.entries(sources)
      .filter(([path, text]) => path !== "./clipboard.ts" && /clipboard\??\.writeText/.test(text))
      .map(([path]) => path);
    expect(writers).toEqual([]);
  });
});
