/**
 * The node header shows the times of the iteration on screen, not always the
 * latest one's.
 */
import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { NodeState } from "../../../shared/generated/NodeState";
import { formatTime } from "../../model/nodeDetailFormat";

vi.mock("../../../shared/extensions/ExtensionActions", () => ({
  ExtensionActionButtons: () => null,
}));

const { default: NodeDetailHeader } = await import("./NodeDetailHeader");

const node = {
  node_id: "worker",
  status: "completed",
  iter: 2,
  started_at: "2026-01-01T10:20:00Z",
  completed_at: "2026-01-01T10:30:00Z",
  iterations: [
    { iter: 1, status: "completed", started_at: "2026-01-01T10:00:00Z", completed_at: "2026-01-01T10:10:00Z" },
    { iter: 2, status: "completed", started_at: "2026-01-01T10:20:00Z", completed_at: "2026-01-01T10:30:00Z" },
  ],
} as unknown as NodeState;

function header(selectedIter: number) {
  render(
    <NodeDetailHeader node={node} hasMultipleIters selectedIter={selectedIter} onSelectIter={() => {}} />,
  );
}

describe("NodeDetailHeader times", () => {
  it("follows the selected iteration", () => {
    header(1);
    expect(screen.getByTestId("node-started-at").textContent).toBe(
      ` · started ${formatTime("2026-01-01T10:00:00Z")}`,
    );
    expect(screen.getByTestId("node-ended-at").textContent).toBe(
      ` · ended ${formatTime("2026-01-01T10:10:00Z")}`,
    );
  });

  it("shows the latest iteration's by default", () => {
    header(2);
    expect(screen.getByTestId("node-started-at").textContent).toContain(formatTime("2026-01-01T10:20:00Z"));
  });
});
