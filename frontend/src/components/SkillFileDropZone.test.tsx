import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import SkillFileDropZone, { DropOverlay } from "./SkillFileDropZone";
import { useFileDropTarget } from "../hooks/useFileDropTarget";

function Host({ onDrop, onEnter }: { onDrop: (dt: DataTransfer) => void; onEnter?: () => void }) {
  const { dragging, handlers } = useFileDropTarget(onDrop, onEnter);
  return (
    <div data-testid="surface" {...handlers} className="relative">
      {dragging !== null && <DropOverlay count={dragging} hint="hint" />}
      <SkillFileDropZone label="Files · drop or" onBrowse={() => {}} onPickFiles={() => {}} />
    </div>
  );
}

const fileDrag = (n: number) => ({
  dataTransfer: { types: ["Files"], items: Array.from({ length: n }, () => ({ kind: "file" })), files: [], dropEffect: "none" },
});

describe("SkillFileDropZone + useFileDropTarget (#671)", () => {
  it("shows the overlay from dragenter on the whole surface, hides it on the last dragleave, drops on the surface", () => {
    const onDrop = vi.fn();
    const onEnter = vi.fn();
    render(<Host onDrop={onDrop} onEnter={onEnter} />);
    const surface = screen.getByTestId("surface");

    fireEvent.dragEnter(surface, fileDrag(3));
    expect(screen.getByTestId("skill-drop-overlay")).toHaveTextContent("Drop to attach 3 files");
    expect(onEnter).toHaveBeenCalledTimes(1);
    // Entering a child bumps the depth; leaving it must not hide the overlay.
    fireEvent.dragEnter(screen.getByTestId("skill-drop-zone"), fileDrag(3));
    fireEvent.dragLeave(screen.getByTestId("skill-drop-zone"), fileDrag(3));
    expect(screen.getByTestId("skill-drop-overlay")).toBeInTheDocument();
    fireEvent.dragLeave(surface, fileDrag(3));
    expect(screen.queryByTestId("skill-drop-overlay")).toBeNull();

    fireEvent.drop(surface, { dataTransfer: { types: ["Files"], items: [], files: [new File(["x"], "a.md")] } });
    expect(onDrop).toHaveBeenCalledTimes(1);
    expect(screen.queryByTestId("skill-drop-overlay")).toBeNull();
  });

  it("ignores drags that are not files (the tree's own row drags)", () => {
    const onDrop = vi.fn();
    render(<Host onDrop={onDrop} />);
    const surface = screen.getByTestId("surface");
    fireEvent.dragEnter(surface, { dataTransfer: { types: ["text/plain"], items: [], files: [] } });
    expect(screen.queryByTestId("skill-drop-overlay")).toBeNull();
    fireEvent.drop(surface, { dataTransfer: { types: ["text/plain"], items: [], files: [] } });
    expect(onDrop).not.toHaveBeenCalled();
  });

  it("swallows window-level file drops while mounted, so a miss never navigates the tab", () => {
    const { unmount } = render(<Host onDrop={() => {}} />);
    const event = new Event("drop", { cancelable: true }) as Event & { dataTransfer: unknown };
    Object.defineProperty(event, "dataTransfer", { value: { types: ["Files"] } });
    window.dispatchEvent(event);
    expect(event.defaultPrevented).toBe(true);
    unmount();
    const after = new Event("drop", { cancelable: true });
    Object.defineProperty(after, "dataTransfer", { value: { types: ["Files"] } });
    window.dispatchEvent(after);
    expect(after.defaultPrevented).toBe(false);
  });

  it("is focusable; Enter and Space open the native picker; Browse… calls onBrowse without opening the picker", () => {
    const onBrowse = vi.fn();
    const onPickFiles = vi.fn();
    render(<SkillFileDropZone label="Files" onBrowse={onBrowse} onPickFiles={onPickFiles} />);
    const zone = screen.getByTestId("skill-drop-zone");
    expect(zone).toHaveAttribute("tabindex", "0");
    const input = screen.getByTestId("skill-drop-zone-input") as HTMLInputElement;
    const click = vi.spyOn(input, "click");
    fireEvent.keyDown(zone, { key: "Enter" });
    fireEvent.keyDown(zone, { key: " " });
    expect(click).toHaveBeenCalledTimes(2);
    fireEvent.click(screen.getByTestId("skill-drop-zone-browse"));
    expect(onBrowse).toHaveBeenCalledTimes(1);
    expect(click).toHaveBeenCalledTimes(2);
    fireEvent.change(input, { target: { files: [new File(["x"], "a.md")] } });
    expect(onPickFiles).toHaveBeenCalledTimes(1);
  });

  it("disabled: not focusable, the picker does not open", () => {
    render(<SkillFileDropZone label="Files" onBrowse={() => {}} onPickFiles={() => {}} disabled />);
    const zone = screen.getByTestId("skill-drop-zone");
    expect(zone).toHaveAttribute("tabindex", "-1");
    const input = screen.getByTestId("skill-drop-zone-input") as HTMLInputElement;
    const click = vi.spyOn(input, "click");
    fireEvent.click(zone);
    expect(click).not.toHaveBeenCalled();
  });
});
