import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import ConflictModal from "./ConflictModal";

describe("ConflictModal", () => {
  const baseProps = {
    open: true,
    pipelineId: "my-pipeline",
    onKeep: vi.fn(),
    onTake: vi.fn(),
  };

  it("renders with pipeline name and both action buttons", () => {
    render(<ConflictModal {...baseProps} />);
    expect(screen.getByText("External edit conflict")).toBeInTheDocument();
    expect(screen.getByText("my-pipeline")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Keep canvas" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Take external" })).toBeInTheDocument();
  });

  it("does not render when open is false", () => {
    render(<ConflictModal {...baseProps} open={false} />);
    expect(screen.queryByText("External edit conflict")).not.toBeInTheDocument();
  });

  it("calls onKeep when 'Keep canvas' is clicked", () => {
    const onKeep = vi.fn();
    render(<ConflictModal {...baseProps} onKeep={onKeep} />);
    fireEvent.click(screen.getByRole("button", { name: "Keep canvas" }));
    expect(onKeep).toHaveBeenCalledTimes(1);
  });

  it("calls onTake when 'Take external' is clicked", () => {
    const onTake = vi.fn();
    render(<ConflictModal {...baseProps} onTake={onTake} />);
    fireEvent.click(screen.getByRole("button", { name: "Take external" }));
    expect(onTake).toHaveBeenCalledTimes(1);
  });

  it("calls onKeep when Escape is pressed", () => {
    const onKeep = vi.fn();
    render(<ConflictModal {...baseProps} onKeep={onKeep} />);
    fireEvent.keyDown(document, { key: "Escape" });
    expect(onKeep).toHaveBeenCalledTimes(1);
  });

  it("calls onKeep when clicking the backdrop", () => {
    const onKeep = vi.fn();
    render(<ConflictModal {...baseProps} onKeep={onKeep} />);
    fireEvent.click(screen.getByTestId("conflict-modal-backdrop"));
    expect(onKeep).toHaveBeenCalledTimes(1);
  });
});
