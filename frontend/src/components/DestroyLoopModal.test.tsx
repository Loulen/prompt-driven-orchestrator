import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import DestroyLoopModal from "./DestroyLoopModal";

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
