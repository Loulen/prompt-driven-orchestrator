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

describe("DestroyLoopModal — bounded to the window (#940)", () => {
  it("scrolls a long loop list in its body; the actions stay outside it", () => {
    const ids = Array.from({ length: 60 }, (_, i) => `loop_${i}`);
    render(<DestroyLoopModal open loopIds={ids} onClose={() => {}} onConfirm={() => {}} />);
    const body = screen.getByTestId("destroy-loop-body");
    expect(body).toHaveClass("min-h-0", "overflow-y-auto");
    expect(body.parentElement).toHaveClass("flex", "flex-col", "max-h-[85vh]");
    expect(body).toHaveTextContent("loop_59");
    expect(body).not.toContainElement(screen.getByTestId("destroy-loop-confirm"));
    expect(screen.getByRole("heading", { name: /destroy these loops/i })).toHaveClass("shrink-0");
  });

  it("keeps closing on a backdrop click", () => {
    const onClose = vi.fn();
    render(<DestroyLoopModal open loopIds={["a"]} onClose={onClose} onConfirm={() => {}} />);
    fireEvent.click(screen.getByTestId("destroy-loop-body"));
    expect(onClose).not.toHaveBeenCalled();
    fireEvent.click(screen.getByTestId("destroy-loop-backdrop"));
    expect(onClose).toHaveBeenCalledOnce();
  });
});
