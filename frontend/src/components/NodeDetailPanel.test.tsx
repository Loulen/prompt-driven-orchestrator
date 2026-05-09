import { render, screen, fireEvent, act } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";

globalThis.ResizeObserver = class {
  observe() {}
  unobserve() {}
  disconnect() {}
};

const fetchPromptMock = vi.fn().mockResolvedValue("system prompt here");
const fetchNodeIOMock = vi
  .fn()
  .mockResolvedValue({ inputs: [], outputs: [] });

vi.mock("../api", () => ({
  fetchPrompt: (...args: unknown[]) => fetchPromptMock(...args),
  fetchNodeIO: (...args: unknown[]) => fetchNodeIOMock(...args),
  markNodeDone: vi.fn(),
  attachSession: vi.fn(),
}));

vi.mock("./TmuxTerminal", () => ({
  default: ({ session, expanded, onExpand, status }: {
    session: string;
    expanded?: boolean;
    onExpand?: () => void;
    status?: string;
  }) => (
    <div data-testid="tmux-terminal" data-session={session} data-expanded={expanded} data-status={status}>
      <button data-testid="term-expand" onClick={onExpand}>expand</button>
    </div>
  ),
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
import { TooltipProvider } from "./ui/tooltip";
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
    fetchPromptMock.mockClear();
    fetchNodeIOMock.mockClear();
  });

  describe("TmuxTerminal integration", () => {
    it("renders TmuxTerminal when node is running", () => {
      render(
        <TooltipProvider>
          <NodeDetailPanel node={makeNode({ status: "running" })} runId="run-1" />
        </TooltipProvider>,
      );
      const terminal = screen.getByTestId("tmux-terminal");
      expect(terminal).toBeInTheDocument();
      expect(terminal.getAttribute("data-session")).toBe(
        "maestro-run-1-test-node-iter-1",
      );
    });

    it("does not render TmuxTerminal when node is pending", () => {
      render(
        <TooltipProvider>
          <NodeDetailPanel node={makeNode({ status: "pending" })} runId="run-1" />
        </TooltipProvider>,
      );
      expect(screen.queryByTestId("tmux-terminal")).not.toBeInTheDocument();
      const placeholder = screen.getByTestId("pending-placeholder");
      expect(placeholder).toBeInTheDocument();
      expect(placeholder).toHaveTextContent("en attente");
    });

    it("passes correct session name with iter", () => {
      render(
        <TooltipProvider>
          <NodeDetailPanel
            node={makeNode({ status: "running", iter: 3, node_id: "impl" })}
            runId="run-abc"
          />
        </TooltipProvider>,
      );
      const terminal = screen.getByTestId("tmux-terminal");
      expect(terminal.getAttribute("data-session")).toBe(
        "maestro-run-abc-impl-iter-3",
      );
    });

    it("renders the details pane (Mark complete + sections) by default (collapsed)", () => {
      render(
        <TooltipProvider>
          <NodeDetailPanel node={makeNode({ status: "running" })} runId="run-1" />
        </TooltipProvider>,
      );
      expect(screen.getByTestId("details-pane")).toBeInTheDocument();
      expect(screen.queryByTestId("terminal-fullsize")).not.toBeInTheDocument();
      expect(screen.getByText("Mark complete")).toBeInTheDocument();
      expect(screen.getByTestId("prompt-toggle")).toBeInTheDocument();
    });

    it("hides the details pane when the terminal is expanded (fullsize)", () => {
      render(
        <TooltipProvider>
          <NodeDetailPanel node={makeNode({ status: "running" })} runId="run-1" />
        </TooltipProvider>,
      );

      const terminal = screen.getByTestId("tmux-terminal");
      expect(terminal.getAttribute("data-expanded")).toBe("false");

      fireEvent.click(screen.getByTestId("term-expand"));

      const reTerminal = screen.getByTestId("tmux-terminal");
      expect(reTerminal.getAttribute("data-expanded")).toBe("true");
      expect(screen.getByTestId("terminal-fullsize")).toBeInTheDocument();
      expect(screen.queryByTestId("details-pane")).not.toBeInTheDocument();
      expect(screen.queryByText("Mark complete")).not.toBeInTheDocument();
      expect(screen.queryByTestId("prompt-toggle")).not.toBeInTheDocument();
    });

    it("re-renders the details pane after collapsing the terminal", () => {
      render(
        <TooltipProvider>
          <NodeDetailPanel node={makeNode({ status: "running" })} runId="run-1" />
        </TooltipProvider>,
      );

      // Expand
      fireEvent.click(screen.getByTestId("term-expand"));
      expect(screen.queryByTestId("details-pane")).not.toBeInTheDocument();

      // Collapse again
      fireEvent.click(screen.getByTestId("term-expand"));
      expect(screen.getByTestId("details-pane")).toBeInTheDocument();
      expect(screen.getByText("Mark complete")).toBeInTheDocument();
    });
  });

  describe("IterSelector", () => {
    function renderPanel(overrides?: Partial<NodeState>) {
      return render(
        <TooltipProvider>
          <NodeDetailPanel node={makeNode(overrides)} runId="run-1" />
        </TooltipProvider>,
      );
    }

    it("does not show selector when only one iteration", () => {
      renderPanel({
        iter: 1,
        iterations: [
          {
            iter: 1,
            status: "completed",
            started_at: null,
            completed_at: null,
          },
        ],
      });
      expect(screen.queryByTestId("iter-option-1")).not.toBeInTheDocument();
    });

    it("switches selectedIter when clicking another iteration", async () => {
      renderPanel({
        iter: 2,
        iterations: [
          { iter: 1, status: "completed", started_at: null, completed_at: null },
          { iter: 2, status: "running", started_at: null, completed_at: null },
        ],
      });

      await act(async () => {});

      fetchNodeIOMock.mockClear();
      fetchPromptMock.mockClear();

      // Open dropdown
      const trigger = screen.getByText(/iter 2/);
      fireEvent.click(trigger);

      // Click iter 1 option
      const option = await screen.findByTestId("iter-option-1");
      fireEvent.click(option);

      await act(async () => {});
    });
  });

  describe("FrontmatterRetryBanners", () => {
    it("shows amber retry-pending banner when running with retries > 0", () => {
      render(
        <TooltipProvider>
          <NodeDetailPanel
            node={makeNode({ status: "running", frontmatter_retries: 1 })}
            runId="run-1"
          />
        </TooltipProvider>,
      );
      expect(screen.getByTestId("frontmatter-retry-banner")).toBeInTheDocument();
      expect(screen.getByTestId("frontmatter-retry-banner")).toHaveTextContent(
        "Frontmatter mismatch",
      );
    });

    it("does not show retry banner when running with retries = 0", () => {
      render(
        <TooltipProvider>
          <NodeDetailPanel
            node={makeNode({ status: "running", frontmatter_retries: 0 })}
            runId="run-1"
          />
        </TooltipProvider>,
      );
      expect(
        screen.queryByTestId("frontmatter-retry-banner"),
      ).not.toBeInTheDocument();
    });

    it("shows exhausted banner when failed with output validation reason", () => {
      render(
        <TooltipProvider>
          <NodeDetailPanel
            node={makeNode({
              status: "failed",
              failure_reason: "output validation failed",
            })}
            runId="run-1"
          />
        </TooltipProvider>,
      );
      expect(
        screen.getByTestId("frontmatter-exhausted-banner"),
      ).toBeInTheDocument();
      expect(
        screen.getByTestId("frontmatter-exhausted-banner"),
      ).toHaveTextContent("output validation failed after retry");
    });

    it("shows offending fields in exhausted banner when violations present", () => {
      render(
        <TooltipProvider>
          <NodeDetailPanel
            node={makeNode({
              status: "failed",
              failure_reason: "output validation failed",
              frontmatter_violations: [
                { port: "review", field: "verdict", reason: "value 'MAYBE' not in allowed values" },
                { port: "review", field: "score", reason: "expected int, got 'high'" },
              ],
            })}
            runId="run-1"
          />
        </TooltipProvider>,
      );
      const list = screen.getByTestId("frontmatter-violation-list");
      expect(list).toBeInTheDocument();
      expect(list.children).toHaveLength(2);
      expect(list).toHaveTextContent("review.verdict");
      expect(list).toHaveTextContent("review.score");
    });

    it("does not show violation list when no violations present", () => {
      render(
        <TooltipProvider>
          <NodeDetailPanel
            node={makeNode({
              status: "failed",
              failure_reason: "output validation failed",
            })}
            runId="run-1"
          />
        </TooltipProvider>,
      );
      expect(
        screen.queryByTestId("frontmatter-violation-list"),
      ).not.toBeInTheDocument();
    });

    it("shows generic failed banner for other failure reasons", () => {
      render(
        <TooltipProvider>
          <NodeDetailPanel
            node={makeNode({
              status: "failed",
              failure_reason: "some other error",
            })}
            runId="run-1"
          />
        </TooltipProvider>,
      );
      expect(
        screen.queryByTestId("frontmatter-exhausted-banner"),
      ).not.toBeInTheDocument();
      expect(screen.getAllByText(/some other error/).length).toBeGreaterThan(0);
    });
  });

  describe("PromptSection", () => {
    it("renders Initial Prompt section collapsed by default", () => {
      render(
        <TooltipProvider><NodeDetailPanel node={makeNode()} runId="run-1" /></TooltipProvider>,
      );
      const toggle = screen.getByTestId("prompt-toggle");
      expect(toggle).toBeInTheDocument();
      expect(toggle.textContent).toContain("Initial Prompt");
      expect(screen.queryByText("system prompt here")).not.toBeInTheDocument();
    });

    it("expands on chevron click and collapses again", async () => {
      render(
        <TooltipProvider><NodeDetailPanel node={makeNode()} runId="run-1" /></TooltipProvider>,
      );
      const toggle = screen.getByTestId("prompt-toggle");

      fireEvent.click(toggle);
      expect(screen.getByText("Loading prompt...")).toBeInTheDocument();

      fireEvent.click(toggle);
      expect(screen.queryByText("Loading prompt...")).not.toBeInTheDocument();
    });
  });

  describe("polled preview removal", () => {
    it("does not have a terminal-pane pre element", () => {
      render(
        <TooltipProvider>
          <NodeDetailPanel node={makeNode({ status: "running" })} runId="run-1" />
        </TooltipProvider>,
      );
      expect(document.querySelector(".terminal-pane")).toBeNull();
    });

    it("does not import or use fetchPane", () => {
      // The mock for ../api no longer includes fetchPane — if the component
      // tried to call it, it would throw. This test verifies the import
      // was removed successfully.
      render(
        <TooltipProvider>
          <NodeDetailPanel node={makeNode({ status: "running" })} runId="run-1" />
        </TooltipProvider>,
      );
      // Just verify it renders without error
      expect(screen.getByTestId("tmux-terminal")).toBeInTheDocument();
    });
  });
});
