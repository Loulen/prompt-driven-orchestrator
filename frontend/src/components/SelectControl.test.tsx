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
    // Shown at rest, hidden on row hover (a ring in the status colour takes its place).
    expect(dot.className).toContain("group-hover:hidden");
    const ring = screen.getByTestId("select-ring");
    expect(ring.className).toContain("group-hover:block");
    expect(ring.className).toContain("rounded-full");
    expect(ring.className).toContain("h-3.5");
    expect(ring.className).toContain("border-2");
    expect(ring.className).toContain("border-current");
    expect(ring.className).toContain("text-st-running");
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

  it("shows a filled accent round with a check when selected, the glyph only for assistive tech", () => {
    render(
      <SelectControl selected status="completed" dotTestId="run-status-dot" label="Deselect run" onSelect={() => {}} testId="ctrl" />,
    );
    const check = screen.getByTestId("select-check");
    expect(check.className).toContain("bg-acc");
    expect(check.className).toContain("rounded-full");
    expect(check.className).toContain("h-4");
    expect(check.className).toContain("text-on-acc");
    expect(screen.getByTestId("run-status-dot").className).toContain("sr-only");
    expect(screen.getByTestId("ctrl")).toHaveAttribute("title", "Deselect run · Completed");
    expect(screen.getByTestId("ctrl")).toHaveAttribute("data-select-mode", "ring");
  });

  it("keeps an unchecked row a visible ring in its status colour while the list is in selection mode", () => {
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
    const ring = screen.getByTestId("select-ring");
    expect(ring.className).toMatch(/(^|\s)block(\s|$)/);
    expect(ring.className).not.toContain("hidden");
    expect(ring.className).toContain("rounded-full");
    expect(ring.className).toContain("text-st-failed");
    expect(screen.getByTestId("run-status-dot").className).toContain("sr-only");
    expect(screen.getByTestId("ctrl")).toHaveAttribute("title", "Select run · Failed: exit 1");
  });

  it("has an empty slot at rest for a row with no status", () => {
    render(<SelectControl selected={false} label="Select pipeline" onSelect={() => {}} testId="ctrl" />);
    expect(screen.queryByRole("img")).not.toBeInTheDocument();
    expect(screen.getByTestId("ctrl")).toHaveAttribute("title", "Select pipeline");
    // Its hover ring is neutral: there is no status colour to lend.
    const ring = screen.getByTestId("select-ring");
    expect(ring.className).toContain("border-fg-3");
    expect(ring.className).not.toContain("border-current");
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
