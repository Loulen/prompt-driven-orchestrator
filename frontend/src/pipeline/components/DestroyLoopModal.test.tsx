import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import DestroyLoopModal from "./DestroyLoopModal";
import { clickBackdrop } from "../../test/clickBackdrop";

describe("DestroyLoopModal (#150)", () => {
  it("renders nothing when closed", () => {
    const { container } = render(
      <DestroyLoopModal open={false} loopIds={["review_loop"]} onClose={() => {}} onConfirm={() => {}} />,
    );
    expect(container.firstChild).toBeNull();
  });

  it("warns that the loop will be destroyed, naming the loop id", () => {
    render(
      <DestroyLoopModal open loopIds={["review_loop"]} onClose={() => {}} onConfirm={() => {}} />,
    );
    // The dialog title makes the destructive intent explicit.
    expect(screen.getByRole("heading", { name: /destroy this loop/i })).toBeInTheDocument();
    expect(screen.getByText("review_loop")).toBeInTheDocument();
  });

  it("calls onConfirm when the destroy button is clicked", () => {
    const onConfirm = vi.fn();
    render(
      <DestroyLoopModal open loopIds={["review_loop"]} onClose={() => {}} onConfirm={onConfirm} />,
    );
    fireEvent.click(screen.getByTestId("destroy-loop-confirm"));
    expect(onConfirm).toHaveBeenCalledOnce();
  });

  it("calls onClose when cancelled", () => {
    const onClose = vi.fn();
    render(
      <DestroyLoopModal open loopIds={["review_loop"]} onClose={onClose} onConfirm={() => {}} />,
    );
    fireEvent.click(screen.getByTestId("destroy-loop-cancel"));
    expect(onClose).toHaveBeenCalledOnce();
  });
});

describe("DestroyLoopModal — bounded to the window (#940)", () => {
  it("lists a long loop list in the dialog's scrolling body; the actions stay outside it", () => {
    const ids = Array.from({ length: 60 }, (_, i) => `loop_${i}`);
    render(<DestroyLoopModal open loopIds={ids} onClose={() => {}} onConfirm={() => {}} />);
    const body = screen.getByTestId("destroy-loop-body");
    expect(body).toHaveTextContent("loop_59");
    expect(body.closest(".d-frame__body")).not.toBeNull();
    expect(body.closest(".d-frame__body")).not.toContainElement(screen.getByTestId("destroy-loop-confirm"));
    expect(screen.getByRole("heading", { name: /destroy these loops/i })).toBeInTheDocument();
  });

  it("keeps closing on a backdrop click, not on a click in its body", () => {
    const onClose = vi.fn();
    render(<DestroyLoopModal open loopIds={["a"]} onClose={onClose} onConfirm={() => {}} />);
    fireEvent.click(screen.getByTestId("destroy-loop-body"));
    expect(onClose).not.toHaveBeenCalled();
    // The backdrop of a modal <dialog> is the dialog itself.
    const backdrop = screen.getByTestId("destroy-loop-modal");
    // A selection pressed in the body and released over the backdrop keeps it open.
    fireEvent.pointerDown(screen.getByTestId("destroy-loop-body"));
    fireEvent.pointerUp(backdrop);
    fireEvent.click(backdrop);
    expect(onClose).not.toHaveBeenCalled();
    clickBackdrop(backdrop);
    expect(onClose).toHaveBeenCalledOnce();
  });
});
