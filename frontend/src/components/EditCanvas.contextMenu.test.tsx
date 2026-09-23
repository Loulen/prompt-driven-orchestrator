import { render, fireEvent, screen } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { ContextMenu } from "./EditCanvas";

function menu(onClose = vi.fn()) {
  render(
    <ContextMenu
      x={10}
      y={10}
      type="edge"
      onDeleteNode={vi.fn()}
      onDeleteNote={vi.fn()}
      onDuplicateNode={vi.fn()}
      onExportNode={vi.fn()}
      onFanOut={vi.fn()}
      onDeleteEdge={vi.fn()}
      onClose={onClose}
    />,
  );
  return onClose;
}

describe("canvas context menu dismissal (#844 FP iter-2)", () => {
  it("offers « Delete edge » on an edge", () => {
    menu();
    expect(screen.getByText("Delete edge")).toBeTruthy();
  });

  it("closes on Escape, so its backdrop does not swallow the next click", () => {
    const onClose = menu();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("ignores other keys", () => {
    const onClose = menu();
    fireEvent.keyDown(window, { key: "a" });
    expect(onClose).not.toHaveBeenCalled();
  });

  it("closes on a right-click outside the menu", () => {
    const onClose = menu();
    fireEvent.contextMenu(screen.getByTestId("context-menu-backdrop"));
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
