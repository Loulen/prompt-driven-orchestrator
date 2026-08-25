import { describe, it, expect, vi } from "vitest";
import type { KeyboardEvent } from "react";
import { handleSelectionKeydown } from "./selectionKeys";

function handlers() {
  return {
    tab: "runs" as const,
    visibleIds: ["a", "b"],
    hasSelection: true,
    selectVisible: vi.fn(),
    clear: vi.fn(),
    onBulkDelete: vi.fn(),
  };
}

function ev(
  over: Partial<{ key: string; ctrlKey: boolean; metaKey: boolean; tag: string; contentEditable: boolean }>,
): KeyboardEvent {
  return {
    key: over.key ?? "x",
    ctrlKey: over.ctrlKey ?? false,
    metaKey: over.metaKey ?? false,
    target: { tagName: over.tag ?? "DIV", isContentEditable: over.contentEditable ?? false },
    preventDefault: vi.fn(),
  } as unknown as KeyboardEvent;
}

describe("handleSelectionKeydown", () => {
  it("Ctrl-A and Cmd-A select every visible id (and preventDefault)", () => {
    const h = handlers();
    const e1 = ev({ key: "a", ctrlKey: true });
    handleSelectionKeydown(e1, h);
    expect(h.selectVisible).toHaveBeenCalledWith("runs", ["a", "b"]);
    expect(e1.preventDefault).toHaveBeenCalled();

    const h2 = handlers();
    handleSelectionKeydown(ev({ key: "A", metaKey: true }), h2);
    expect(h2.selectVisible).toHaveBeenCalledWith("runs", ["a", "b"]);
  });

  it("Ctrl-A is a no-op when there is nothing visible", () => {
    const h = { ...handlers(), visibleIds: [] };
    handleSelectionKeydown(ev({ key: "a", ctrlKey: true }), h);
    expect(h.selectVisible).not.toHaveBeenCalled();
  });

  it("Escape clears only when something is selected", () => {
    const h = handlers();
    handleSelectionKeydown(ev({ key: "Escape" }), h);
    expect(h.clear).toHaveBeenCalledWith("runs");

    const empty = { ...handlers(), hasSelection: false };
    handleSelectionKeydown(ev({ key: "Escape" }), empty);
    expect(empty.clear).not.toHaveBeenCalled();
  });

  it("Delete opens the bulk-delete confirm only when something is selected", () => {
    const h = handlers();
    handleSelectionKeydown(ev({ key: "Delete" }), h);
    expect(h.onBulkDelete).toHaveBeenCalled();

    const empty = { ...handlers(), hasSelection: false };
    handleSelectionKeydown(ev({ key: "Delete" }), empty);
    expect(empty.onBulkDelete).not.toHaveBeenCalled();
  });

  it("ignores every shortcut while typing in a field", () => {
    for (const target of [
      { tag: "INPUT" },
      { tag: "TEXTAREA" },
      { tag: "DIV", contentEditable: true },
    ]) {
      const h = handlers();
      handleSelectionKeydown(ev({ key: "a", ctrlKey: true, ...target }), h);
      handleSelectionKeydown(ev({ key: "Escape", ...target }), h);
      handleSelectionKeydown(ev({ key: "Delete", ...target }), h);
      expect(h.selectVisible).not.toHaveBeenCalled();
      expect(h.clear).not.toHaveBeenCalled();
      expect(h.onBulkDelete).not.toHaveBeenCalled();
    }
  });
});
