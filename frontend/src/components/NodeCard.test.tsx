import { render, screen } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import { NodeCard } from "./NodeCard";
import type { NodeStatus } from "../types";

describe("NodeCard", () => {
  const statuses: NodeStatus[] = ["pending", "running", "awaiting_user", "completed", "failed"];

  it("renders with 4-side border (no border-l-[3px])", () => {
    const { container } = render(
      <NodeCard status="running"><span>child</span></NodeCard>,
    );
    const card = container.firstElementChild!;
    expect(card.className).toContain("border-[1.5px]");
    expect(card.className).not.toContain("border-l-[3px]");
  });

  it.each(statuses)("status %s applies the correct border class", (status) => {
    const { container } = render(
      <NodeCard status={status}><span>child</span></NodeCard>,
    );
    const card = container.firstElementChild!;
    if (status === "pending") {
      expect(card.className).toContain("border-line-strong");
    } else if (status === "running") {
      expect(card.className).toContain("border-st-running");
    } else if (status === "awaiting_user") {
      expect(card.className).toContain("border-st-await");
    } else if (status === "completed") {
      expect(card.className).toContain("border-st-done");
    } else if (status === "failed") {
      expect(card.className).toContain("border-st-failed");
    }
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
