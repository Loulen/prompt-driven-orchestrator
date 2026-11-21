import { describe, it, expect, vi } from "vitest";
import type { Terminal } from "@xterm/xterm";
import { wirePointerAndKeys } from "./xtermPointer";

/** A Terminal double holding the custom key handler that wirePointerAndKeys attaches. */
function fakeTerminal() {
  let handler: ((ev: KeyboardEvent) => boolean) | undefined;
  const term = {
    modes: { mouseTrackingMode: "none" },
    hasSelection: () => false,
    clearSelection: vi.fn(),
    input: vi.fn(),
    attachCustomKeyEventHandler: (h: (ev: KeyboardEvent) => boolean) => {
      handler = h;
    },
  };
  wirePointerAndKeys(term as unknown as Terminal, document.createElement("div"));
  return { term, press: (ev: KeyboardEvent) => handler!(ev) };
}

const enter = (type: string, init: KeyboardEventInit = {}) =>
  new KeyboardEvent(type, { key: "Enter", cancelable: true, ...init });

describe("wirePointerAndKeys key handler (CORE-284)", () => {
  it("sends ESC CR once for Shift+Enter and keeps xterm from sending \\r", () => {
    const { term, press } = fakeTerminal();
    const down = enter("keydown", { shiftKey: true });
    expect(press(down)).toBe(false);
    expect(down.defaultPrevented).toBe(true);
    expect(press(enter("keypress", { shiftKey: true }))).toBe(false);
    expect(press(enter("keyup", { shiftKey: true }))).toBe(false);
    expect(term.input).toHaveBeenCalledTimes(1);
    expect(term.input).toHaveBeenCalledWith("\x1b\r");
  });

  it("leaves plain Enter and Alt+Enter to xterm", () => {
    const { term, press } = fakeTerminal();
    expect(press(enter("keydown"))).toBe(true);
    expect(press(enter("keydown", { altKey: true }))).toBe(true);
    expect(term.input).not.toHaveBeenCalled();
  });
});
