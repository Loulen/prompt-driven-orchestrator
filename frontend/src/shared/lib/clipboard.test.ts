import { afterEach, describe, expect, it, vi } from "vitest";
import { copyImageToClipboard, copyToClipboard } from "./clipboard";

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

describe("copyImageToClipboard", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    Reflect.deleteProperty(window, "ClipboardItem");
    Reflect.deleteProperty(navigator, "clipboard");
  });

  it("writes a PNG through the async Clipboard API", async () => {
    const write = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", { value: { write }, configurable: true });
    class Item {
      items: Record<string, Promise<Blob>>;
      constructor(items: Record<string, Promise<Blob>>) {
        this.items = items;
      }
    }
    Object.defineProperty(window, "ClipboardItem", { value: Item, configurable: true });
    const png = new Blob(["x"], { type: "image/png" });
    expect(await copyImageToClipboard(png)).toBe(true);
    const [[[item]]] = write.mock.calls;
    expect(await item.items["image/png"]).toBe(png);
  });

  it("says false where no Clipboard API carries an image (plain http)", async () => {
    Object.defineProperty(navigator, "clipboard", { value: undefined, configurable: true });
    expect(await copyImageToClipboard(new Blob(["x"], { type: "image/png" }))).toBe(false);
  });

  it("says false when the write is refused", async () => {
    Object.defineProperty(navigator, "clipboard", { value: { write: vi.fn().mockRejectedValue(new Error("denied")) }, configurable: true });
    Object.defineProperty(window, "ClipboardItem", { value: class {}, configurable: true });
    expect(await copyImageToClipboard(new Blob(["x"], { type: "image/png" }))).toBe(false);
  });
});
