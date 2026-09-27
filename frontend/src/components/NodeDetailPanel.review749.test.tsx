import { render, screen } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";

// #749: the node panel's shortcut to the Review page. There is no "node diff"
// surface (ADR-0067 §1): the panel only opens the Review with this node's
// delivery preselected, and only when there is something to review.

globalThis.ResizeObserver = class {
  observe() {}
  unobserve() {}
  disconnect() {}
};

vi.mock("../api", () => ({
  artifactUrl: (runId: string, path: string) => `/runs/${runId}/artifact?path=${encodeURIComponent(path)}`,
}));
vi.mock("../run/api/commands", () => ({
  killNode: vi.fn(),
  restartNode: vi.fn(),
}));
vi.mock("../run/api/completion", () => ({
  markNodeDone: vi.fn().mockResolvedValue({ kind: "completed" }),
}));
vi.mock("../run/api/nodes", () => ({
  fetchPrompt: vi.fn().mockResolvedValue("prompt"),
  fetchNodeIO: vi.fn().mockResolvedValue({ inputs: [], outputs: [] }),
  stopNode: vi.fn(),
  retryNode: vi.fn(),
  retryNodePreview: vi.fn().mockResolvedValue({ downstream: [], affected_count: 0, with_artifacts: [] }),
  startNode: vi.fn(),
}));

vi.mock("../workspace/api/provisioning", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../workspace/api/provisioning")>()),
  previewProvisioning: vi.fn().mockResolvedValue({ entries: [], rules: [], conflicts: [] }),
}));

vi.mock("../session/components/TmuxTerminal", () => ({
  default: () => <div data-testid="tmux-terminal" />,
}));

vi.mock("../shared/ui/resizable", () => ({
  ResizablePanelGroup: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
  ResizablePanel: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
  ResizableHandle: () => <div />,
}));

vi.mock("./MarkdownArtifactModal", () => ({ default: () => null }));

import NodeDetailPanel from "./NodeDetailPanel";
import type { NodeState } from "../shared/generated/NodeState";

function makeNode(overrides?: Partial<NodeState>): NodeState {
  return { frontmatter_retries: 0,
    node_id: "impl",
    status: "completed",
    iter: 1,
    started_at: "2026-09-09T00:00:00Z",
    completed_at: "2026-09-09T00:01:00Z",
    failure_reason: null,
    iterations: [],
    ...overrides,
  };
}

describe("NodeDetailPanel — Review shortcut (#749)", () => {
  it("offers « Review this node's delivery » for a delivered node, linking to its before → after", () => {
    render(
      <NodeDetailPanel
        node={makeNode({ delivery: { before: "aaa", after: "bbb" }, iter: 2 })}
        runId="run-1"
        nodeName="Implement"
      />,
    );
    const link = screen.getByTestId("node-review-shortcut");
    expect(link).toHaveTextContent("Review this node's delivery");
    expect(link).toHaveAttribute("href", "/runs/run-1/review?from=node%3Aimpl%3A2%3Abefore&to=node%3Aimpl%3A2%3Aafter");
  });

  it("offers « Review live changes » for a running isolated node, tip → live", () => {
    render(
      <NodeDetailPanel
        node={makeNode({ status: "running", completed_at: null, isolated_worktree: true })}
        runId="run-1"
      />,
    );
    const link = screen.getByTestId("node-review-shortcut");
    expect(link).toHaveTextContent("Review live changes");
    expect(link).toHaveAttribute("href", "/runs/run-1/review?from=tip&to=live%3Aimpl");
  });

  it("offers nothing for a node with nothing to review", () => {
    const { unmount } = render(<NodeDetailPanel node={makeNode({ status: "pending", completed_at: null })} runId="run-1" />);
    expect(screen.queryByTestId("node-review-shortcut")).toBeNull();
    unmount();
    // Running in the shared worktree: no live branch of its own.
    render(<NodeDetailPanel node={makeNode({ status: "running", completed_at: null, isolated_worktree: false })} runId="run-1" />);
    expect(screen.queryByTestId("node-review-shortcut")).toBeNull();
  });

  it("offers nothing on an archived Run (the branch is gone)", () => {
    render(
      <NodeDetailPanel node={makeNode({ delivery: { before: "aaa", after: "bbb" } })} runId="run-1" isArchived />,
    );
    expect(screen.queryByTestId("node-review-shortcut")).toBeNull();
  });
});
