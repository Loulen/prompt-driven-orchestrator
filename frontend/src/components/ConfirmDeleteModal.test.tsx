import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import ConfirmDeleteModal from "./ConfirmDeleteModal";

describe("ConfirmDeleteModal", () => {
  const baseProps = {
    open: true,
    onClose: vi.fn(),
    onConfirm: vi.fn(),
    name: "my-pipeline",
  };

  it("renders title with default kind 'pipeline'", () => {
    render(<ConfirmDeleteModal {...baseProps} />);
    expect(screen.getByText("Delete this pipeline?")).toBeInTheDocument();
  });

  it("renders name in monospace", () => {
    render(<ConfirmDeleteModal {...baseProps} />);
    const nameEl = screen.getByText("my-pipeline");
    expect(nameEl).toBeInTheDocument();
    expect(nameEl.tagName).toBe("CODE");
  });

  it("renders custom kind in title", () => {
    render(<ConfirmDeleteModal {...baseProps} kind="library" />);
    expect(screen.getByText("Delete this library?")).toBeInTheDocument();
  });

  it("renders custom detail text", () => {
    render(<ConfirmDeleteModal {...baseProps} detail="Custom warning text." />);
    expect(screen.getByText("Custom warning text.")).toBeInTheDocument();
  });

  it("does not render when open is false", () => {
    render(<ConfirmDeleteModal {...baseProps} open={false} />);
    expect(screen.queryByText("Delete this pipeline?")).not.toBeInTheDocument();
  });

  it("calls onClose when Cancel is clicked", () => {
    const onClose = vi.fn();
    render(<ConfirmDeleteModal {...baseProps} onClose={onClose} />);
    fireEvent.click(screen.getByText("Cancel"));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("calls onConfirm when Delete is clicked", () => {
    const onConfirm = vi.fn();
    render(<ConfirmDeleteModal {...baseProps} onConfirm={onConfirm} />);
    fireEvent.click(screen.getByText("Delete"));
    expect(onConfirm).toHaveBeenCalledTimes(1);
  });

  it("calls onClose when Escape is pressed", () => {
    const onClose = vi.fn();
    render(<ConfirmDeleteModal {...baseProps} onClose={onClose} />);
    fireEvent.keyDown(document, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("calls onConfirm when Enter is pressed", () => {
    const onConfirm = vi.fn();
    render(<ConfirmDeleteModal {...baseProps} onConfirm={onConfirm} />);
    fireEvent.keyDown(document, { key: "Enter" });
    expect(onConfirm).toHaveBeenCalledTimes(1);
  });

  it("calls onClose when clicking the backdrop", () => {
    const onClose = vi.fn();
    render(<ConfirmDeleteModal {...baseProps} onClose={onClose} />);
    const backdrop = screen.getByTestId("confirm-delete-backdrop");
    fireEvent.click(backdrop);
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  // #227 — the opt-in cascade checkbox renders only when `cascadeLabel` is set
  // and its state flows through onConfirm(cascade).
  describe("cascade checkbox (#227)", () => {
    it("renders no checkbox by default", () => {
      render(<ConfirmDeleteModal {...baseProps} />);
      expect(screen.queryByTestId("delete-cascade-checkbox")).not.toBeInTheDocument();
    });

    it("renders the checkbox (unchecked) with its label when cascadeLabel is set", () => {
      render(<ConfirmDeleteModal {...baseProps} cascadeLabel="Also remove the Library copy" />);
      const box = screen.getByTestId("delete-cascade-checkbox") as HTMLInputElement;
      expect(box).toBeInTheDocument();
      expect(box.checked).toBe(false);
      expect(screen.getByText("Also remove the Library copy")).toBeInTheDocument();
    });

    it("calls onConfirm(false) when Delete is clicked without ticking", () => {
      const onConfirm = vi.fn();
      render(
        <ConfirmDeleteModal {...baseProps} onConfirm={onConfirm} cascadeLabel="Also remove the Library copy" />,
      );
      fireEvent.click(screen.getByText("Delete"));
      expect(onConfirm).toHaveBeenCalledWith(false);
    });

    it("calls onConfirm(true) when Delete is clicked after ticking", () => {
      const onConfirm = vi.fn();
      render(
        <ConfirmDeleteModal {...baseProps} onConfirm={onConfirm} cascadeLabel="Also remove the Library copy" />,
      );
      fireEvent.click(screen.getByTestId("delete-cascade-checkbox"));
      fireEvent.click(screen.getByText("Delete"));
      expect(onConfirm).toHaveBeenCalledWith(true);
    });

    it("passes the latest checkbox state through the Enter key", () => {
      const onConfirm = vi.fn();
      render(
        <ConfirmDeleteModal {...baseProps} onConfirm={onConfirm} cascadeLabel="Also remove the Library copy" />,
      );
      fireEvent.click(screen.getByTestId("delete-cascade-checkbox"));
      fireEvent.keyDown(document, { key: "Enter" });
      expect(onConfirm).toHaveBeenCalledWith(true);
    });
  });
});
