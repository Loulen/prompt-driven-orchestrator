import { render, screen } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import { NodeCard } from "./NodeCard";
import type { NodeStatus } from "../types";

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
    ["failed", "border-st-failed"],
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

  it("failed nodes show the red overlay badge at top-right", () => {
    render(<NodeCard status="failed"><span>child</span></NodeCard>);
    const badge = screen.getByTestId("failed-badge");
    expect(badge).toBeInTheDocument();
    expect(badge.className).toContain("bg-st-failed");
    expect(badge.className).toContain("rounded-full");
    expect(badge.style.top).toBe("-7px");
    expect(badge.style.right).toBe("-7px");
  });

  it("non-failed nodes do not show the badge", () => {
    render(<NodeCard status="running"><span>child</span></NodeCard>);
    expect(screen.queryByTestId("failed-badge")).not.toBeInTheDocument();
  });

  it("selected nodes show the gap-offset emerald ring via box-shadow", () => {
    const { container } = render(
      <NodeCard status="running" selected><span>child</span></NodeCard>,
    );
    const card = container.firstElementChild as HTMLElement;
    expect(card.style.boxShadow).toContain("var(--color-bg-1)");
    expect(card.style.boxShadow).toContain("var(--color-acc)");
  });

  it("selected + failed shows both the ring and the badge", () => {
    const { container } = render(
      <NodeCard status="failed" selected><span>child</span></NodeCard>,
    );
    const card = container.firstElementChild as HTMLElement;
    expect(card.style.boxShadow).toContain("var(--color-acc)");
    expect(screen.getByTestId("failed-badge")).toBeInTheDocument();
  });

  it("unselected nodes have no box-shadow style", () => {
    const { container } = render(
      <NodeCard status="completed"><span>child</span></NodeCard>,
    );
    const card = container.firstElementChild as HTMLElement;
    expect(card.style.boxShadow).toBe("");
  });
});
