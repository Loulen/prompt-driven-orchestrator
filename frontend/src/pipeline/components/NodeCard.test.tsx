import { render } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import { NodeCard } from "./NodeCard";
import type { NodeStatus } from "../../shared/generated/NodeStatus";

function card(status: NodeStatus | null, selected?: boolean): HTMLElement {
  const { container } = render(
    <NodeCard status={status} selected={selected}>
      <span>child</span>
    </NodeCard>,
  );
  return container.firstElementChild as HTMLElement;
}

describe("NodeCard", () => {
  it.each<[NodeStatus, string]>([
    ["running", "running"],
    ["awaiting_user", "awaiting"],
    ["completed", "done"],
    // #620: a pruned node is greyed, distinct from the green "done" — a
    // short-circuited branch must not read as a branch that ran.
    ["skipped", "skipped"],
    ["failed", "failed"],
    ["stopped", "skipped"],
    ["stale", "skipped"],
    ["interrupted", "awaiting"],
  ])("tints a %s node in the %s tone", (status, tone) => {
    expect(card(status).getAttribute("data-st")).toBe(tone);
  });

  it("draws a pending node, and a template's, without a status tint", () => {
    expect(card("pending").hasAttribute("data-st")).toBe(false);
    expect(card("waiting").hasAttribute("data-st")).toBe(false);
    expect(card(null).hasAttribute("data-st")).toBe(false);
  });

  it("marks the selection, once, whatever the status", () => {
    expect(card("failed", true).getAttribute("data-selected")).toBe("true");
    expect(card("completed").hasAttribute("data-selected")).toBe(false);
  });
});
