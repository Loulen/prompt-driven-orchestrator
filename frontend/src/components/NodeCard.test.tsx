import { render, screen } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import { NodeCard } from "./NodeCard";
import type { NodeStatus } from "../types";
import { statusSpec } from "../lib/statusTable";

describe("NodeCard", () => {
  it("renders with 4-side border (no border-l-[3px])", () => {
    const { container } = render(
      <NodeCard status="running"><span>child</span></NodeCard>,
    );
    const card = container.firstElementChild!;
    expect(card.className).toContain("border-[1.5px]");
    expect(card.className).not.toContain("border-l-[3px]");
  });

  it.each<[NodeStatus, string]>([
    ["pending", "border-line-strong"],
    ["running", "border-st-running"],
    ["awaiting_user", "border-st-await"],
    ["completed", "border-st-done"],
    // #620: a pruned node wears a greyed "skipped" cadre, distinct from the green
    // "done" one — a short-circuited branch must not read as a branch that ran.
    ["skipped", "border-st-skipped"],
    ["failed", "border-st-failed"],
    ["stopped", "border-st-stopped"],
    ["stale", "border-st-stale"],
  ])("status %s applies border class %s", (status, expectedClass) => {
    const { container } = render(
      <NodeCard status={status}><span>child</span></NodeCard>,
    );
    expect(container.firstElementChild!.className).toContain(expectedClass);
  });

  it("pending renders without a status-colored cadre", () => {
    const { container } = render(
      <NodeCard status="pending"><span>child</span></NodeCard>,
    );
    const card = container.firstElementChild!;
    expect(card.className).toContain("border-line-strong");
    expect(card.className).not.toContain("border-st-pending");
  });

  // PDO-22 (#1000): the Status icon replaces the red failed badge, in the corner.
  it.each<[Exclude<NodeStatus, "pending">, string, string]>([
    ["running", "LoaderCircle", "Running"],
    ["awaiting_user", "CircleHelp", "Awaiting you"],
    ["completed", "CircleCheck", "Completed"],
    ["failed", "CircleX", "Failed"],
    ["skipped", "CircleArrowRight", "Skipped"],
    ["stopped", "CircleStop", "Stopped"],
    ["stale", "Clock", "Stale"],
    ["interrupted", "CircleSlash", "Interrupted"],
  ])("a %s node wears its %s glyph in the top-right corner, named and titled", (status, glyph, label) => {
    render(<NodeCard status={status}><span>child</span></NodeCard>);
    const badge = screen.getByTestId("node-status-badge");
    expect(badge.className).toContain("bg-bg-1");
    expect(badge.className).toContain("rounded-full");
    expect(badge.style.top).toBe("-9px");
    expect(badge.style.right).toBe("-9px");
    const icon = screen.getByTestId("node-status-icon");
    expect(icon).toHaveAttribute("role", "img");
    expect(icon).toHaveAttribute("aria-label", label);
    expect(icon).toHaveAttribute("title", label);
    expect(icon).toHaveAttribute("data-status", status);
    // The node's glyph is the list's one: the single STATUS_TABLE entry.
    expect(statusSpec(status).glyphName).toBe(glyph);
    expect(icon.querySelector("svg")).not.toBeNull();
  });

  it("the red failed badge is gone", () => {
    render(<NodeCard status="failed"><span>child</span></NodeCard>);
    expect(screen.queryByTestId("failed-badge")).not.toBeInTheDocument();
    expect(screen.getByTestId("node-status-badge").className).not.toContain("bg-st-failed");
  });

  it("a pending node — a library template's node included — wears no glyph", () => {
    render(<NodeCard status="pending"><span>child</span></NodeCard>);
    expect(screen.queryByTestId("node-status-badge")).not.toBeInTheDocument();
    expect(screen.queryByRole("img")).not.toBeInTheDocument();
  });

  it("selected nodes show the gap-offset emerald ring via box-shadow", () => {
    const { container } = render(
      <NodeCard status="running" selected><span>child</span></NodeCard>,
    );
    const card = container.firstElementChild as HTMLElement;
    expect(card.style.boxShadow).toContain("var(--color-bg-1)");
    expect(card.style.boxShadow).toContain("var(--color-acc)");
  });

  it("selected + failed shows both the ring and the glyph", () => {
    const { container } = render(
      <NodeCard status="failed" selected><span>child</span></NodeCard>,
    );
    const card = container.firstElementChild as HTMLElement;
    expect(card.style.boxShadow).toContain("var(--color-acc)");
    expect(screen.getByTestId("node-status-icon")).toHaveAttribute("data-status", "failed");
  });

  it("unselected nodes have no box-shadow style", () => {
    const { container } = render(
      <NodeCard status="completed"><span>child</span></NodeCard>,
    );
    const card = container.firstElementChild as HTMLElement;
    expect(card.style.boxShadow).toBe("");
  });
});
