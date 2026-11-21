import { describe, it, expect } from "vitest";
import { ALT_ENTER, newlineKeyAction } from "./terminalKeys";

describe("newlineKeyAction (CORE-284)", () => {
  const key = (type: string, init: KeyboardEventInit = {}) =>
    new KeyboardEvent(type, { key: "Enter", ...init });

  it("turns Shift+Enter's keydown into a new line", () => {
    expect(newlineKeyAction(key("keydown", { shiftKey: true }))).toBe("newline");
  });

  it("swallows Shift+Enter's keypress, else xterm sends \\r after the new line", () => {
    expect(newlineKeyAction(key("keypress", { shiftKey: true }))).toBe("swallow");
  });

  it("swallows Shift+Enter's keyup", () => {
    expect(newlineKeyAction(key("keyup", { shiftKey: true }))).toBe("swallow");
  });

  it("lets plain Enter through: it still submits", () => {
    expect(newlineKeyAction(key("keydown"))).toBe("pass");
    expect(newlineKeyAction(key("keypress"))).toBe("pass");
  });

  it("lets the other Enter combinations through unchanged", () => {
    expect(newlineKeyAction(key("keydown", { altKey: true }))).toBe("pass");
    expect(newlineKeyAction(key("keydown", { shiftKey: true, ctrlKey: true }))).toBe("pass");
    expect(newlineKeyAction(key("keydown", { shiftKey: true, metaKey: true }))).toBe("pass");
    expect(newlineKeyAction(key("keydown", { shiftKey: true, altKey: true }))).toBe("pass");
  });

  it("does not hijack an IME composition commit", () => {
    expect(newlineKeyAction(key("keydown", { shiftKey: true, isComposing: true }))).toBe("pass");
  });

  it("ignores other shifted keys", () => {
    expect(newlineKeyAction(key("keydown", { key: "A", shiftKey: true }))).toBe("pass");
    expect(newlineKeyAction(key("keydown", { key: "Tab", shiftKey: true }))).toBe("pass");
  });

  it("sends Alt+Enter's sequence, ESC then CR", () => {
    expect(ALT_ENTER).toBe("\x1b\r");
  });
});
