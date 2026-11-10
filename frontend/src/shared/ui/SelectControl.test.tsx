import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import SelectControl from "./SelectControl";

describe("SelectControl", () => {
  it("renders the resting status dot with its colour, pulse, tooltip and test id", () => {
    render(
      <SelectControl
        selected={false}
        dotClass="bg-st-running"
        pulse
        dotTitle="still merging"
        dotTestId="run-status-dot"
        label="Select run"
        onSelect={() => {}}
      />,
    );
    const dot = screen.getByTestId("run-status-dot");
    expect(dot.className).toContain("bg-st-running");
    expect(dot.className).toContain("animate-pulse");
    expect(dot).toHaveAttribute("title", "still merging");
    // not selected ⇒ no check glyph
    expect(screen.queryByTestId("select-check")).not.toBeInTheDocument();
  });

  it("shows a check (and no status dot) when selected", () => {
    render(
      <SelectControl selected dotClass="bg-st-done" dotTestId="run-status-dot" label="Deselect run" onSelect={() => {}} />,
    );
    expect(screen.getByTestId("select-check")).toBeInTheDocument();
    expect(screen.queryByTestId("run-status-dot")).not.toBeInTheDocument();
  });

  it("exposes an accessible checkbox reflecting the selected state", () => {
    const { rerender } = render(
      <SelectControl selected={false} label="Select run" onSelect={() => {}} testId="ctrl" />,
    );
    expect(screen.getByTestId("ctrl")).toHaveAttribute("aria-checked", "false");
    rerender(<SelectControl selected label="Deselect run" onSelect={() => {}} testId="ctrl" />);
    expect(screen.getByTestId("ctrl")).toHaveAttribute("aria-checked", "true");
  });

  it("calls onSelect and stops the click from reaching the row", () => {
    const onSelect = vi.fn();
    const rowClick = vi.fn();
    render(
      <button onClick={rowClick}>
        <SelectControl selected={false} label="Select run" onSelect={onSelect} testId="ctrl" />
      </button>,
    );
    fireEvent.click(screen.getByTestId("ctrl"));
    expect(onSelect).toHaveBeenCalledTimes(1);
    expect(rowClick).not.toHaveBeenCalled();
  });

  it("forwards the shift modifier so the caller can extend a range", () => {
    const onSelect = vi.fn();
    render(<SelectControl selected={false} label="Select run" onSelect={onSelect} testId="ctrl" />);
    fireEvent.click(screen.getByTestId("ctrl"), { shiftKey: true });
    expect(onSelect.mock.calls[0][0].shiftKey).toBe(true);
  });
});
