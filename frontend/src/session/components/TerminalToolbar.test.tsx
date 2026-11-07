import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, within } from "@testing-library/react";
import TerminalToolbar from "./TerminalToolbar";

type Props = Parameters<typeof TerminalToolbar>[0];

function renderBar(over: Partial<Props> = {}) {
  const props: Props = {
    session: "pdo-run1-design-iter-1",
    statusLabel: "attached · live",
    connected: true,
    spectating: false,
    takenOver: false,
    watchers: 0,
    hasSelection: false,
    copyFeedback: null,
    expanded: false,
    onCopy: vi.fn(),
    onTakeControl: vi.fn(),
    ...over,
  };
  render(<TerminalToolbar {...props} />);
  return props;
}

describe("TerminalToolbar's pilot / spectator chip", () => {
  it("reads « You pilot » with a ruby dot, and nothing after it when nobody watches", () => {
    renderBar();
    const chip = screen.getByTestId("term-mode");
    expect(chip).toHaveAttribute("data-role", "pilot");
    expect(chip).toHaveTextContent(/^You pilot$/);
    expect(chip.querySelector(".ri-term-bar__role-dot")).not.toBeNull();
    expect(chip.querySelector(".ri-term-bar__role-sep")).toBeNull();
    expect(screen.queryByTestId("term-watchers")).toBeNull();
  });

  it("adds « N watching » after a rule when other browsers watch", () => {
    renderBar({ watchers: 2 });
    const chip = screen.getByTestId("term-mode");
    expect(chip.querySelector(".ri-term-bar__role-sep")).not.toBeNull();
    const watchers = within(chip).getByTestId("term-watchers");
    expect(watchers).toHaveTextContent("2 watching");
    expect(watchers).toHaveAttribute("aria-label", "Watched from 2 other browsers");
  });

  it("reads « Watching » for a spectator, and takes control from the chip", () => {
    const props = renderBar({ spectating: true });
    const chip = screen.getByTestId("term-mode");
    expect(chip).toHaveAttribute("data-role", "spectator");
    expect(chip).toHaveTextContent("Watching");
    const take = within(chip).getByRole("button", { name: "Take control" });
    expect(take).toHaveClass("ri-term-bar__role-action");
    fireEvent.click(take);
    expect(props.onTakeControl).toHaveBeenCalledTimes(1);
    // The lone hand icon button is gone: the action lives in the chip only.
    expect(screen.getAllByRole("button", { name: "Take control" })).toHaveLength(1);
  });

  it("keeps the « taken over » notice next to the chip", () => {
    renderBar({ spectating: true, takenOver: true });
    expect(screen.getByTestId("term-taken-over")).toHaveTextContent("Another browser took control");
  });

  it("names a state with no driver plainly, never in capitals", () => {
    renderBar({ connected: false, statusLabel: "connecting…" });
    const tag = screen.getByTestId("term-mode");
    expect(tag).not.toHaveAttribute("data-role");
    expect(tag).toHaveTextContent("connecting…");
    expect(screen.queryByText(/^(PILOT|SPECTATOR)$/)).toBeNull();
    expect(screen.queryByText(/^(pilot|spectator)$/)).toBeNull();
  });
});
