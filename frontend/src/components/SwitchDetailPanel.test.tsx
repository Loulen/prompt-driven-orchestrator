import { render, screen } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";

const fetchNodeIOMock = vi
  .fn()
  .mockResolvedValue({ inputs: [], outputs: [] });

vi.mock("../api", () => ({
  fetchNodeIO: (...args: unknown[]) => fetchNodeIOMock(...args),
}));

vi.mock("./MarkdownArtifactModal", () => ({
  default: () => null,
}));

import SwitchDetailPanel from "./SwitchDetailPanel";
import type { NodeState, SwitchStateInfo } from "../types";

function makeNode(overrides?: Partial<NodeState>): NodeState {
  return {
    node_id: "sw-1",
    status: "completed",
    iter: 1,
    started_at: "2026-01-01T00:00:00Z",
    completed_at: "2026-01-01T00:00:00Z",
    failure_reason: null,
    iterations: [],
    ...overrides,
  };
}

function makeSwitchState(overrides?: Partial<SwitchStateInfo>): SwitchStateInfo {
  return {
    switch_node_id: "sw-1",
    chosen_branch: "pass",
    evaluated_at: "2026-01-01T00:00:01Z",
    ...overrides,
  };
}

describe("SwitchDetailPanel", () => {
  beforeEach(() => {
    fetchNodeIOMock.mockClear();
  });

  it("renders chosen branch when switchState is provided", () => {
    render(
      <SwitchDetailPanel
        node={makeNode()}
        runId="run-1"
        switchState={makeSwitchState({ chosen_branch: "pass" })}
        nodeName="Review Gate"
      />,
    );
    expect(screen.getByTestId("switch-detail-panel")).toBeInTheDocument();
    expect(screen.getByTestId("switch-chosen-branch")).toBeInTheDocument();
    expect(screen.getByText("pass")).toBeInTheDocument();
    expect(screen.getByText("Review Gate")).toBeInTheDocument();
  });

  it("shows pending message when no switchState", () => {
    render(
      <SwitchDetailPanel
        node={makeNode({ status: "pending" })}
        runId="run-1"
        switchState={null}
      />,
    );
    expect(
      screen.getByText(/Waiting for upstream/),
    ).toBeInTheDocument();
  });

  it("displays Switch badge instead of status label", () => {
    render(
      <SwitchDetailPanel
        node={makeNode()}
        runId="run-1"
        switchState={makeSwitchState()}
      />,
    );
    expect(screen.getByText("Switch")).toBeInTheDocument();
  });

  it("does not render terminal or prompt sections", () => {
    render(
      <SwitchDetailPanel
        node={makeNode()}
        runId="run-1"
        switchState={makeSwitchState()}
      />,
    );
    expect(screen.queryByTestId("tmux-terminal")).not.toBeInTheDocument();
    expect(screen.queryByText("Initial Prompt")).not.toBeInTheDocument();
  });

  it("fetches I/O for completed switch node", () => {
    render(
      <SwitchDetailPanel
        node={makeNode()}
        runId="run-1"
        switchState={makeSwitchState()}
      />,
    );
    expect(fetchNodeIOMock).toHaveBeenCalledWith("run-1", "sw-1", 1);
  });

  it("shows evaluated-at timestamp", () => {
    render(
      <SwitchDetailPanel
        node={makeNode()}
        runId="run-1"
        switchState={makeSwitchState({ evaluated_at: "2026-01-01T12:30:45Z" })}
      />,
    );
    expect(screen.getByText(/evaluated/)).toBeInTheDocument();
  });
});
