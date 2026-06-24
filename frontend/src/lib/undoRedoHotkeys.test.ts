import { describe, it, expect, vi, afterEach } from "vitest";
import { handleUndoRedoKeydown } from "./undoRedoHotkeys";

// Build a KeyboardEvent-like object with a spy-able preventDefault so we can
// assert on it without relying on the real event's read-only `defaultPrevented`.
function key(
  k: string,
  mods: { ctrl?: boolean; meta?: boolean; shift?: boolean } = {},
): KeyboardEvent {
  return {
    key: k,
    ctrlKey: mods.ctrl ?? false,
    metaKey: mods.meta ?? false,
    shiftKey: mods.shift ?? false,
    preventDefault: vi.fn(),
  } as unknown as KeyboardEvent;
}

describe("handleUndoRedoKeydown (ADR-0014 / #226)", () => {
  afterEach(() => {
    document.body.innerHTML = "";
  });

  it("Ctrl+Z invokes undo (not redo) and prevents default", () => {
    const undo = vi.fn();
    const redo = vi.fn();
    const e = key("z", { ctrl: true });
    handleUndoRedoKeydown(e, undo, redo);
    expect(undo).toHaveBeenCalledTimes(1);
    expect(redo).not.toHaveBeenCalled();
    expect(e.preventDefault).toHaveBeenCalled();
  });

  it("Cmd+Z (metaKey) also invokes undo", () => {
    const undo = vi.fn();
    const redo = vi.fn();
    handleUndoRedoKeydown(key("z", { meta: true }), undo, redo);
    expect(undo).toHaveBeenCalledTimes(1);
  });

  it("Ctrl+Shift+Z invokes redo", () => {
    const undo = vi.fn();
    const redo = vi.fn();
    handleUndoRedoKeydown(key("z", { ctrl: true, shift: true }), undo, redo);
    expect(redo).toHaveBeenCalledTimes(1);
    expect(undo).not.toHaveBeenCalled();
  });

  it("handles the uppercase 'Z' some layouts emit with Shift", () => {
    const undo = vi.fn();
    const redo = vi.fn();
    handleUndoRedoKeydown(key("Z", { ctrl: true, shift: true }), undo, redo);
    expect(redo).toHaveBeenCalledTimes(1);
  });

  it("Ctrl+Y invokes redo", () => {
    const undo = vi.fn();
    const redo = vi.fn();
    handleUndoRedoKeydown(key("y", { ctrl: true }), undo, redo);
    expect(redo).toHaveBeenCalledTimes(1);
    expect(undo).not.toHaveBeenCalled();
  });

  it("a bare Z (no modifier) does nothing", () => {
    const undo = vi.fn();
    const redo = vi.fn();
    const e = key("z");
    handleUndoRedoKeydown(e, undo, redo);
    expect(undo).not.toHaveBeenCalled();
    expect(redo).not.toHaveBeenCalled();
    expect(e.preventDefault).not.toHaveBeenCalled();
  });

  it("yields to native field undo: Ctrl+Z while an INPUT is focused does nothing", () => {
    const input = document.createElement("input");
    document.body.appendChild(input);
    input.focus();
    expect(document.activeElement).toBe(input);

    const undo = vi.fn();
    const redo = vi.fn();
    const e = key("z", { ctrl: true });
    handleUndoRedoKeydown(e, undo, redo);
    expect(undo).not.toHaveBeenCalled();
    expect(e.preventDefault).not.toHaveBeenCalled();
  });

  it("yields while a TEXTAREA is focused", () => {
    const ta = document.createElement("textarea");
    document.body.appendChild(ta);
    ta.focus();
    const undo = vi.fn();
    handleUndoRedoKeydown(key("z", { ctrl: true }), undo, vi.fn());
    expect(undo).not.toHaveBeenCalled();
  });

  it("yields while a contentEditable element is focused", () => {
    const div = document.createElement("div");
    div.contentEditable = "true";
    // jsdom only treats a <div> as focusable if it has a tabindex; real browsers
    // focus a contenteditable div without one. The guard itself keys on
    // `contentEditable === "true"` (jsdom returns undefined for isContentEditable).
    div.tabIndex = 0;
    document.body.appendChild(div);
    div.focus();
    expect(document.activeElement).toBe(div);
    const undo = vi.fn();
    handleUndoRedoKeydown(key("z", { ctrl: true }), undo, vi.fn());
    expect(undo).not.toHaveBeenCalled();
  });
});
