import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import SelectControl from "./SelectControl";

describe("SelectControl", () => {
  it("renders the resting Status icon with its label, reason and test id", () => {
    render(
      <SelectControl
        selected={false}
        status="running"
        statusReason="still merging"
        dotTestId="run-status-dot"
        label="Select run"
        onSelect={() => {}}
        testId="ctrl"
      />,
    );
    const dot = screen.getByTestId("run-status-dot");
    expect(dot).toHaveAttribute("role", "img");
    expect(dot).toHaveAttribute("aria-label", "Running");
    expect(dot).toHaveAttribute("title", "Running: still merging");
    // Shown at rest, hidden on row hover (the empty square takes its place).
    expect(dot.className).toContain("group-hover:hidden");
    expect(screen.getByTestId("select-square").className).toContain("group-hover:block");
    expect(screen.getByTestId("select-square").className).toContain("rounded-[2.5px]");
    // The status stays readable while the glyph is hidden: it rides the control's tooltip.
    expect(screen.getByTestId("ctrl")).toHaveAttribute("title", "Select run · Running: still merging");
    // not selected ⇒ no check
    expect(screen.queryByTestId("select-check")).not.toBeInTheDocument();
  });

  it("keeps the glyph a sibling of the checkbox so screen readers announce the status", () => {
    render(<SelectControl selected={false} status="failed" label="Select run" onSelect={() => {}} testId="ctrl" />);
    const img = screen.getByRole("img", { name: "Failed" });
    expect(screen.getByTestId("ctrl").contains(img)).toBe(false);
    expect(screen.getByRole("checkbox", { name: "Select run" })).toBeInTheDocument();
  });

  it("shows a filled accent square with a check when selected, the glyph only for assistive tech", () => {
    render(
      <SelectControl selected status="completed" dotTestId="run-status-dot" label="Deselect run" onSelect={() => {}} testId="ctrl" />,
    );
    const check = screen.getByTestId("select-check");
    expect(check.className).toContain("bg-acc");
    expect(check.className).toContain("rounded-[2.5px]");
    expect(check.className).not.toContain("rounded-full");
    expect(screen.getByTestId("run-status-dot").className).toContain("sr-only");
    expect(screen.getByTestId("ctrl")).toHaveAttribute("title", "Deselect run · Completed");
    expect(screen.getByTestId("ctrl")).toHaveAttribute("data-select-mode", "square");
  });

  it("keeps an unchecked row a visible empty square while the list is in selection mode", () => {
    render(
      <SelectControl
        selected={false}
        selecting
        status="failed"
        statusReason="exit 1"
        dotTestId="run-status-dot"
        label="Select run"
        onSelect={() => {}}
        testId="ctrl"
      />,
    );
    const square = screen.getByTestId("select-square");
    expect(square.className).toMatch(/(^|\s)block(\s|$)/);
    expect(square.className).not.toContain("hidden");
    expect(screen.getByTestId("run-status-dot").className).toContain("sr-only");
    expect(screen.getByTestId("ctrl")).toHaveAttribute("title", "Select run · Failed: exit 1");
  });

  it("has an empty slot at rest for a row with no status", () => {
    render(<SelectControl selected={false} label="Select pipeline" onSelect={() => {}} testId="ctrl" />);
    expect(screen.queryByRole("img")).not.toBeInTheDocument();
    expect(screen.getByTestId("ctrl")).toHaveAttribute("title", "Select pipeline");
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
