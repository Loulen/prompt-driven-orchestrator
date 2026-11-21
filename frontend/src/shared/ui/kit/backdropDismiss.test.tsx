import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import { useBackdropDismiss } from "./backdropDismiss";
import { clickBackdrop } from "../../../test/clickBackdrop";

function Backdrop({
  onDismiss,
  enabled,
  stopPropagation,
}: {
  onDismiss: () => void;
  enabled?: boolean;
  stopPropagation?: boolean;
}) {
  const handlers = useBackdropDismiss(onDismiss, enabled, { stopPropagation });
  return (
    <div data-testid="backdrop" {...handlers}>
      <div data-testid="content">text</div>
    </div>
  );
}

const backdrop = () => screen.getByTestId("backdrop");
const content = () => screen.getByTestId("content");

describe("useBackdropDismiss", () => {
  it("T1: a press and a release on the backdrop dismiss", () => {
    const onDismiss = vi.fn();
    render(<Backdrop onDismiss={onDismiss} />);
    clickBackdrop(backdrop());
    expect(onDismiss).toHaveBeenCalledTimes(1);
  });

  it("T2: a press inside released on the backdrop (a selection) keeps it open", () => {
    const onDismiss = vi.fn();
    render(<Backdrop onDismiss={onDismiss} />);
    fireEvent.pointerDown(content());
    fireEvent.pointerUp(backdrop());
    fireEvent.click(backdrop());
    expect(onDismiss).not.toHaveBeenCalled();
  });

  it("T3: a press on the backdrop released inside keeps it open", () => {
    const onDismiss = vi.fn();
    render(<Backdrop onDismiss={onDismiss} />);
    fireEvent.pointerDown(backdrop());
    fireEvent.pointerUp(content());
    fireEvent.click(backdrop());
    expect(onDismiss).not.toHaveBeenCalled();
  });

  it("T4: a bare click with no press keeps it open", () => {
    const onDismiss = vi.fn();
    render(<Backdrop onDismiss={onDismiss} />);
    fireEvent.click(backdrop());
    expect(onDismiss).not.toHaveBeenCalled();
  });

  it("T5: an earlier backdrop press leaves no flag behind", () => {
    const onDismiss = vi.fn();
    render(<Backdrop onDismiss={onDismiss} />);
    fireEvent.pointerDown(backdrop());
    fireEvent.pointerUp(document.body);
    fireEvent.pointerDown(content());
    fireEvent.pointerUp(backdrop());
    fireEvent.click(backdrop());
    expect(onDismiss).not.toHaveBeenCalled();
  });

  it("T6: disabled, a genuine click does nothing", () => {
    const onDismiss = vi.fn();
    render(<Backdrop onDismiss={onDismiss} enabled={false} />);
    clickBackdrop(backdrop());
    expect(onDismiss).not.toHaveBeenCalled();
  });

  it("T7: stopPropagation keeps the click from a parent backdrop", () => {
    const onDismiss = vi.fn();
    const parent = vi.fn();
    render(
      <div onClick={parent}>
        <Backdrop onDismiss={onDismiss} stopPropagation />
      </div>,
    );
    clickBackdrop(backdrop());
    expect(onDismiss).toHaveBeenCalledTimes(1);
    expect(parent).not.toHaveBeenCalled();
  });

  it("a click inside the content with stopPropagation still reaches nothing above", () => {
    const parent = vi.fn();
    render(
      <div onClick={parent}>
        <Backdrop onDismiss={() => {}} stopPropagation />
      </div>,
    );
    fireEvent.click(content());
    expect(parent).not.toHaveBeenCalled();
  });
});
