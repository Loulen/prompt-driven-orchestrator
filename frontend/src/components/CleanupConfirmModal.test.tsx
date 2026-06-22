import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import CleanupConfirmModal from "./CleanupConfirmModal";

// A realistic run id: `slice(-7)` === "abc1234".
const RUN_ID = "20260622-091516-abc1234";
const SHORT_ID = "abc1234";

describe("CleanupConfirmModal", () => {
  describe("live run (gated)", () => {
    it("renders the 'hasn't finished' consequence copy and the expected short id", () => {
      render(
        <CleanupConfirmModal
          runId={RUN_ID}
          isLive
          onConfirm={vi.fn()}
          onCancel={vi.fn()}
        />,
      );
      expect(screen.getByText(/hasn't finished/)).toBeInTheDocument();
      // The expected short id is shown inline so the friction is derivable on screen.
      expect(screen.getByText(SHORT_ID)).toBeInTheDocument();
    });

    it("disables the confirm button on mount and a stray click does not archive", () => {
      const onConfirm = vi.fn();
      render(
        <CleanupConfirmModal
          runId={RUN_ID}
          isLive
          onConfirm={onConfirm}
          onCancel={vi.fn()}
        />,
      );
      const button = screen.getByTestId("cleanup-confirm-button");
      expect(button).toBeDisabled();
      fireEvent.click(button);
      expect(onConfirm).not.toHaveBeenCalled();
    });

    it("keeps the button disabled for the wrong text, enables it for the short id", () => {
      const onConfirm = vi.fn();
      render(
        <CleanupConfirmModal
          runId={RUN_ID}
          isLive
          onConfirm={onConfirm}
          onCancel={vi.fn()}
        />,
      );
      const input = screen.getByTestId("cleanup-confirm-input");
      const button = screen.getByTestId("cleanup-confirm-button");

      fireEvent.change(input, { target: { value: "wrong" } });
      expect(button).toBeDisabled();
      fireEvent.click(button);
      expect(onConfirm).not.toHaveBeenCalled();

      fireEvent.change(input, { target: { value: SHORT_ID } });
      expect(button).toBeEnabled();
      fireEvent.click(button);
      expect(onConfirm).toHaveBeenCalledTimes(1);
    });

    it("submits on Enter only once the short id is typed", () => {
      const onConfirm = vi.fn();
      render(
        <CleanupConfirmModal
          runId={RUN_ID}
          isLive
          onConfirm={onConfirm}
          onCancel={vi.fn()}
        />,
      );
      const input = screen.getByTestId("cleanup-confirm-input");

      fireEvent.keyDown(input, { key: "Enter" });
      expect(onConfirm).not.toHaveBeenCalled();

      fireEvent.change(input, { target: { value: SHORT_ID } });
      fireEvent.keyDown(input, { key: "Enter" });
      expect(onConfirm).toHaveBeenCalledTimes(1);
    });
  });

  describe("terminal run (light)", () => {
    it("renders the light copy with no typed gate and confirms on a single click", () => {
      const onConfirm = vi.fn();
      render(
        <CleanupConfirmModal
          runId={RUN_ID}
          onConfirm={onConfirm}
          onCancel={vi.fn()}
        />,
      );
      expect(screen.getByText(/remove worktrees and artifacts/)).toBeInTheDocument();
      // No typed-confirmation input on the terminal branch.
      expect(screen.queryByTestId("cleanup-confirm-input")).toBeNull();

      const button = screen.getByTestId("cleanup-confirm-button");
      expect(button).toBeEnabled();
      fireEvent.click(button);
      expect(onConfirm).toHaveBeenCalledTimes(1);
    });
  });

  it("calls onCancel from the cancel button and never confirms on render", () => {
    const onConfirm = vi.fn();
    const onCancel = vi.fn();
    render(
      <CleanupConfirmModal
        runId={RUN_ID}
        isLive
        onConfirm={onConfirm}
        onCancel={onCancel}
      />,
    );
    expect(onConfirm).not.toHaveBeenCalled();
    fireEvent.click(screen.getByText("Cancel"));
    expect(onCancel).toHaveBeenCalledTimes(1);
  });
});
