import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import EffortPicker from "./EffortPicker";

// Spelled out on purpose (as in ModelPicker.test.tsx): the component keeps its
// list module-local, so this is an independent statement of the curated set — a
// silent change to either side fails here rather than agreeing with itself.
const EFFORT_LEVELS = ["low", "medium", "high", "xhigh", "max"] as const;

describe("EffortPicker (#424)", () => {
  it("renders Default plus the five curated levels as radios in a named group", () => {
    render(<EffortPicker value={null} onChange={() => {}} testid="node-effort" />);

    // The `Field` label carries no `htmlFor` and wraps nothing, so the group's
    // accessible name has to come from `aria-label` — this is what the agentic
    // browser test drives.
    const group = screen.getByRole("radiogroup", { name: "Effort" });
    expect(group).toBeInTheDocument();
    expect(screen.getAllByRole("radio")).toHaveLength(1 + EFFORT_LEVELS.length);
    expect(screen.getByTestId("node-effort-option-default")).toBeInTheDocument();
    for (const l of EFFORT_LEVELS) {
      expect(screen.getByTestId(`node-effort-option-${l}`)).toBeInTheDocument();
    }
  });

  it("is not a slider — a range input is undrivable by the agentic test", () => {
    const { container } = render(
      <EffortPicker value="high" onChange={() => {}} testid="node-effort" />,
    );
    expect(container.querySelectorAll('input[type="range"]')).toHaveLength(0);
  });

  it("marks Default checked when the value is unset", () => {
    render(<EffortPicker value={null} onChange={() => {}} testid="node-effort" />);
    expect(screen.getByTestId("node-effort-option-default")).toHaveAttribute(
      "aria-checked",
      "true",
    );
    for (const l of EFFORT_LEVELS) {
      expect(screen.getByTestId(`node-effort-option-${l}`)).toHaveAttribute(
        "aria-checked",
        "false",
      );
    }
  });

  it("treats an empty string like unset (Default checked, no empty --effort)", () => {
    render(<EffortPicker value="" onChange={() => {}} testid="node-effort" />);
    expect(screen.getByTestId("node-effort-option-default")).toHaveAttribute(
      "aria-checked",
      "true",
    );
    expect(screen.queryByTestId("node-effort-option-passthrough")).toBeNull();
  });

  it("marks the current level checked, and only that one", () => {
    render(<EffortPicker value="xhigh" onChange={() => {}} testid="node-effort" />);
    expect(screen.getByTestId("node-effort-option-xhigh")).toHaveAttribute(
      "aria-checked",
      "true",
    );
    expect(screen.getByTestId("node-effort-option-default")).toHaveAttribute(
      "aria-checked",
      "false",
    );
    expect(
      screen.getAllByRole("radio").filter((r) => r.getAttribute("aria-checked") === "true"),
    ).toHaveLength(1);
  });

  it("clicking a level calls onChange with that level", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(<EffortPicker value={null} onChange={onChange} testid="node-effort" />);

    await user.click(screen.getByTestId("node-effort-option-low"));

    // Spelled as the single argument it is: vitest compares arity strictly, so a
    // second parameter appearing later would silently pass here otherwise.
    expect(onChange).toHaveBeenCalledWith("low");
    expect(onChange).toHaveBeenCalledTimes(1);
  });

  it("clicking Default calls onChange(null) — unset, never an empty string", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(<EffortPicker value="max" onChange={onChange} testid="node-effort" />);

    await user.click(screen.getByTestId("node-effort-option-default"));

    expect(onChange).toHaveBeenCalledWith(null);
  });

  it("shows an unknown level in a dedicated pass-through segment, checked", () => {
    // The wire is free-text: a hand-authored `effort: turbo` must stay visible and
    // un-clobbered (ADR-0001, clarification #268). The failure mode this closes is
    // "nothing selected" — or worse, Default reading as selected while the file
    // says otherwise.
    render(<EffortPicker value="turbo" onChange={() => {}} testid="node-effort" />);

    const extra = screen.getByTestId("node-effort-option-passthrough");
    expect(extra).toHaveTextContent("turbo");
    expect(extra).toHaveAttribute("aria-checked", "true");
    expect(screen.getByTestId("node-effort-option-default")).toHaveAttribute(
      "aria-checked",
      "false",
    );
    expect(screen.getAllByRole("radio")).toHaveLength(2 + EFFORT_LEVELS.length);
  });

  it("shows no pass-through segment for a known level", () => {
    render(<EffortPicker value="medium" onChange={() => {}} testid="node-effort" />);
    expect(screen.queryByTestId("node-effort-option-passthrough")).toBeNull();
  });

  it("honours the testid prefix so the merge inspector gets its own handles", () => {
    render(<EffortPicker value="low" onChange={() => {}} testid="merge-effort" />);
    expect(screen.getByTestId("merge-effort-option-low")).toBeInTheDocument();
    expect(screen.queryByTestId("node-effort-option-low")).toBeNull();
  });
});
