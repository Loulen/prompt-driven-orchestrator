import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import EffortPicker from "./EffortPicker";

// #616/ADR-0053: the levels are SERVED, passed via `efforts` — no module-local
// list. This stands in for a harness's deduced effort catalogue.
const EFFORTS = ["low", "medium", "high", "xhigh", "max"];

describe("EffortPicker (#424, #616)", () => {
  it("renders Default plus the SERVED levels as radios in a named group", () => {
    render(<EffortPicker value={null} onChange={() => {}} efforts={EFFORTS} testid="node-effort" />);

    // The `Field` label carries no `htmlFor` and wraps nothing, so the group's
    // accessible name has to come from `aria-label` — this is what the agentic
    // browser test drives.
    const group = screen.getByRole("radiogroup", { name: "Effort" });
    expect(group).toBeInTheDocument();
    expect(screen.getAllByRole("radio")).toHaveLength(1 + EFFORTS.length);
    expect(screen.getByTestId("node-effort-option-default")).toBeInTheDocument();
    for (const l of EFFORTS) {
      expect(screen.getByTestId(`node-effort-option-${l}`)).toBeInTheDocument();
    }
  });

  it("renders as many stops as the binary offers — even seven (AC #4)", () => {
    // A harness may enumerate more effort stops than claude, including ones claude
    // has no name for. The picker renders whatever is served — here seven, beside
    // the always-present unset Default segment.
    const seven = ["min", "low", "medium", "high", "max", "ultra", "turbo"];
    render(<EffortPicker value={null} onChange={() => {}} efforts={seven} testid="node-effort" />);
    for (const l of seven) {
      expect(screen.getByTestId(`node-effort-option-${l}`)).toBeInTheDocument();
    }
    expect(screen.getAllByRole("radio")).toHaveLength(1 + seven.length);
  });

  it("is not a slider — a range input is undrivable by the agentic test", () => {
    const { container } = render(
      <EffortPicker value="high" onChange={() => {}} efforts={EFFORTS} testid="node-effort" />,
    );
    expect(container.querySelectorAll('input[type="range"]')).toHaveLength(0);
  });

  it("marks Default checked when the value is unset", () => {
    render(<EffortPicker value={null} onChange={() => {}} efforts={EFFORTS} testid="node-effort" />);
    expect(screen.getByTestId("node-effort-option-default")).toHaveAttribute("aria-checked", "true");
    for (const l of EFFORTS) {
      expect(screen.getByTestId(`node-effort-option-${l}`)).toHaveAttribute("aria-checked", "false");
    }
  });

  it("treats an empty string like unset (Default checked, no empty --effort)", () => {
    render(<EffortPicker value="" onChange={() => {}} efforts={EFFORTS} testid="node-effort" />);
    expect(screen.getByTestId("node-effort-option-default")).toHaveAttribute("aria-checked", "true");
    expect(screen.queryByTestId("node-effort-option-passthrough")).toBeNull();
  });

  it("marks the current level checked, and only that one", () => {
    render(<EffortPicker value="xhigh" onChange={() => {}} efforts={EFFORTS} testid="node-effort" />);
    expect(screen.getByTestId("node-effort-option-xhigh")).toHaveAttribute("aria-checked", "true");
    expect(screen.getByTestId("node-effort-option-default")).toHaveAttribute("aria-checked", "false");
    expect(
      screen.getAllByRole("radio").filter((r) => r.getAttribute("aria-checked") === "true"),
    ).toHaveLength(1);
  });

  it("clicking a level calls onChange with that level", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(<EffortPicker value={null} onChange={onChange} efforts={EFFORTS} testid="node-effort" />);

    await user.click(screen.getByTestId("node-effort-option-low"));

    expect(onChange).toHaveBeenCalledWith("low");
    expect(onChange).toHaveBeenCalledTimes(1);
  });

  it("clicking Default calls onChange(null) — unset, never an empty string", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(<EffortPicker value="max" onChange={onChange} efforts={EFFORTS} testid="node-effort" />);

    await user.click(screen.getByTestId("node-effort-option-default"));

    expect(onChange).toHaveBeenCalledWith(null);
  });

  it("shows an unknown level in a dedicated pass-through segment, checked", () => {
    // The wire is free-text: a hand-authored `effort: turbo` (outside the served
    // set) must stay visible and un-clobbered (ADR-0001, clarification #268).
    render(<EffortPicker value="turbo" onChange={() => {}} efforts={EFFORTS} testid="node-effort" />);

    const extra = screen.getByTestId("node-effort-option-passthrough");
    expect(extra).toHaveTextContent("turbo");
    expect(extra).toHaveAttribute("aria-checked", "true");
    expect(screen.getByTestId("node-effort-option-default")).toHaveAttribute("aria-checked", "false");
    expect(screen.getAllByRole("radio")).toHaveLength(2 + EFFORTS.length);
  });

  it("shows no pass-through segment for a served level", () => {
    render(<EffortPicker value="medium" onChange={() => {}} efforts={EFFORTS} testid="node-effort" />);
    expect(screen.queryByTestId("node-effort-option-passthrough")).toBeNull();
  });

  it("honours the testid prefix so the merge inspector gets its own handles", () => {
    render(<EffortPicker value="low" onChange={() => {}} efforts={EFFORTS} testid="merge-effort" />);
    expect(screen.getByTestId("merge-effort-option-low")).toBeInTheDocument();
    expect(screen.queryByTestId("node-effort-option-low")).toBeNull();
  });

  // #616/ADR-0053: greyed when the resolved harness has no effort axis (served fact).
  it("is greyed when disabled — every option carries the `disabled` attribute", () => {
    render(<EffortPicker value={null} onChange={() => {}} efforts={EFFORTS} testid="node-effort" disabled />);
    // Assert the DISABLED attribute, never `.value`: a `.value` assertion on a
    // control cannot fail (the known trap this AC calls out).
    for (const radio of screen.getAllByRole("radio")) {
      expect(radio).toBeDisabled();
    }
    expect(screen.getByRole("radiogroup", { name: "Effort" })).toHaveAttribute("aria-disabled", "true");
  });

  it("does not fire onChange while disabled", async () => {
    const onChange = vi.fn();
    render(<EffortPicker value={null} onChange={onChange} efforts={EFFORTS} testid="node-effort" disabled />);
    await userEvent.click(screen.getByTestId("node-effort-option-high"));
    expect(onChange).not.toHaveBeenCalled();
  });

  it("is enabled and fires onChange by default (no `disabled` prop)", async () => {
    const onChange = vi.fn();
    render(<EffortPicker value={null} onChange={onChange} efforts={EFFORTS} testid="node-effort" />);
    const high = screen.getByTestId("node-effort-option-high");
    expect(high).not.toBeDisabled();
    await userEvent.click(high);
    expect(onChange).toHaveBeenCalledWith("high");
  });
});
