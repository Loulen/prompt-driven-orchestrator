import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";

globalThis.ResizeObserver = class {
  observe() {}
  unobserve() {}
  disconnect() {}
};

vi.mock("../api", () => ({
  fetchPane: vi.fn().mockResolvedValue({ content: "" }),
  fetchPrompt: vi.fn().mockResolvedValue("system prompt here"),
  fetchNodeIO: vi.fn().mockResolvedValue({ inputs: [], outputs: [] }),
  markNodeDone: vi.fn(),
  attachSession: vi.fn(),
}));

vi.mock("ansi-to-html", () => ({
  default: class {
    toHtml(s: string) {
      return s;
    }
  },
}));

vi.mock("./ui/resizable", () => ({
  ResizablePanelGroup: ({ children }: { children: React.ReactNode }) => (
    <div>{children}</div>
  ),
  ResizablePanel: ({ children }: { children: React.ReactNode }) => (
    <div>{children}</div>
  ),
  ResizableHandle: () => <div />,
}));

vi.mock("./MarkdownArtifactModal", () => ({
  default: () => null,
}));

import NodeDetailPanel from "./NodeDetailPanel";
import type { NodeState } from "../types";

function makeNode(overrides?: Partial<NodeState>): NodeState {
  return {
    node_id: "test-node",
    status: "running",
    iter: 1,
    started_at: "2026-01-01T00:00:00Z",
    completed_at: null,
    failure_reason: null,
    iterations: [],
    ...overrides,
  };
}

describe("NodeDetailPanel", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  describe("PromptSection", () => {
    it("renders Initial Prompt section collapsed by default", () => {
      render(
        <NodeDetailPanel node={makeNode()} runId="run-1" />,
      );
      const toggle = screen.getByTestId("prompt-toggle");
      expect(toggle).toBeInTheDocument();
      expect(toggle.textContent).toContain("Initial Prompt");
      expect(screen.queryByText("system prompt here")).not.toBeInTheDocument();
    });

    it("expands on chevron click and collapses again", async () => {
      render(
        <NodeDetailPanel node={makeNode()} runId="run-1" />,
      );
      const toggle = screen.getByTestId("prompt-toggle");

      fireEvent.click(toggle);
      expect(screen.getByText("Loading prompt...")).toBeInTheDocument();

      fireEvent.click(toggle);
      expect(screen.queryByText("Loading prompt...")).not.toBeInTheDocument();
    });
  });
});
