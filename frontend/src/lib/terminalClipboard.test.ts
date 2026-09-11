import { describe, it, expect, vi } from "vitest";
import {
  clipboardKeyAction,
  forcedSelectionEvent,
  isMacPlatform,
  writeClipboardText,
} from "./terminalClipboard";

describe("forcedSelectionEvent (#772)", () => {
  const down = (init: MouseEventInit = {}) =>
    new MouseEvent("mousedown", { bubbles: true, cancelable: true, button: 0, ...init });

  it("returns null when the pty does not track the mouse", () => {
    expect(forcedSelectionEvent(down(), { mouseTrackingActive: false, isMac: false })).toBeNull();
  });

  it("adds Shift on non-mac so xterm forces a browser selection", () => {
    const clone = forcedSelectionEvent(down({ clientX: 7, detail: 2 }), {
      mouseTrackingActive: true,
      isMac: false,
    });
    expect(clone).not.toBeNull();
    expect(clone!.shiftKey).toBe(true);
    expect(clone!.altKey).toBe(false);
    expect(clone!.clientX).toBe(7);
    // Double/triple click detection lives in `detail`.
    expect(clone!.detail).toBe(2);
  });

  it("adds Option on mac (with macOptionClickForcesSelection) instead of Shift", () => {
    const clone = forcedSelectionEvent(down(), { mouseTrackingActive: true, isMac: true });
    expect(clone!.altKey).toBe(true);
    expect(clone!.shiftKey).toBe(false);
  });

  it("does not rewrite an event that already carries the modifier", () => {
    expect(
      forcedSelectionEvent(down({ shiftKey: true }), { mouseTrackingActive: true, isMac: false }),
    ).toBeNull();
    expect(
      forcedSelectionEvent(down({ altKey: true }), { mouseTrackingActive: true, isMac: true }),
    ).toBeNull();
  });

  it("never rewrites its own clone (no infinite re-dispatch)", () => {
    const clone = forcedSelectionEvent(down(), { mouseTrackingActive: true, isMac: false })!;
    expect(forcedSelectionEvent(clone, { mouseTrackingActive: true, isMac: false })).toBeNull();
  });

  it("lets middle click through to the pty", () => {
    expect(
      forcedSelectionEvent(down({ button: 1 }), { mouseTrackingActive: true, isMac: false }),
    ).toBeNull();
  });

  it("forces selection for right click too (context-menu paste)", () => {
    const clone = forcedSelectionEvent(down({ button: 2 }), {
      mouseTrackingActive: true,
      isMac: false,
    });
    expect(clone!.button).toBe(2);
    expect(clone!.shiftKey).toBe(true);
  });
});

describe("clipboardKeyAction (#772)", () => {
  const key = (init: KeyboardEventInit) => new KeyboardEvent("keydown", init);

  it("Ctrl+C copies only when there is a selection", () => {
    expect(clipboardKeyAction(key({ key: "c", ctrlKey: true }), { hasSelection: true, isMac: false })).toBe("copy");
    expect(clipboardKeyAction(key({ key: "c", ctrlKey: true }), { hasSelection: false, isMac: false })).toBe("pass");
  });

  it("Ctrl+V always pastes", () => {
    expect(clipboardKeyAction(key({ key: "v", ctrlKey: true }), { hasSelection: false, isMac: false })).toBe("paste");
  });

  it("leaves Ctrl+Shift+C/V to xterm's own handling", () => {
    expect(clipboardKeyAction(key({ key: "C", ctrlKey: true, shiftKey: true }), { hasSelection: true, isMac: false })).toBe("pass");
    expect(clipboardKeyAction(key({ key: "V", ctrlKey: true, shiftKey: true }), { hasSelection: false, isMac: false })).toBe("pass");
  });

  it("uses Cmd on mac and ignores Ctrl there", () => {
    expect(clipboardKeyAction(key({ key: "c", metaKey: true }), { hasSelection: true, isMac: true })).toBe("copy");
    expect(clipboardKeyAction(key({ key: "c", ctrlKey: true }), { hasSelection: true, isMac: true })).toBe("pass");
  });

  it("ignores keyup and other keys", () => {
    expect(clipboardKeyAction(new KeyboardEvent("keyup", { key: "c", ctrlKey: true }), { hasSelection: true, isMac: false })).toBe("pass");
    expect(clipboardKeyAction(key({ key: "x", ctrlKey: true }), { hasSelection: true, isMac: false })).toBe("pass");
  });
});

describe("isMacPlatform", () => {
  it("detects mac from platform or userAgent", () => {
    expect(isMacPlatform({ platform: "MacIntel", userAgent: "" })).toBe(true);
    expect(isMacPlatform({ platform: "", userAgent: "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15)" })).toBe(true);
    expect(isMacPlatform({ platform: "Linux x86_64", userAgent: "Mozilla/5.0 (X11; Linux)" })).toBe(false);
  });
});

describe("writeClipboardText (#772)", () => {
  it("prefers navigator.clipboard when present", async () => {
    const writeText = vi.fn(async () => undefined);
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    const execCommand = vi.fn(() => true);
    Object.defineProperty(document, "execCommand", { value: execCommand, configurable: true });
    expect(await writeClipboardText("abc")).toBe(true);
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
    expect(await writeClipboardText("from-vps")).toBe(true);
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
    expect(await writeClipboardText("x")).toBe(true);
    expect(execCommand).toHaveBeenCalledWith("copy");
  });

  it("reports failure when neither path works", async () => {
    Object.defineProperty(navigator, "clipboard", { value: undefined, configurable: true });
    Object.defineProperty(document, "execCommand", { value: vi.fn(() => false), configurable: true });
    expect(await writeClipboardText("x")).toBe(false);
  });
});
