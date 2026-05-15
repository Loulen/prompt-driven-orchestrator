import { render, screen, fireEvent, act } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { useEffect } from "react";

globalThis.ResizeObserver = class {
  observe() {}
  unobserve() {}
  disconnect() {}
};

const fetchPromptMock = vi.fn().mockResolvedValue("system prompt here");
const fetchNodeIOMock = vi
  .fn()
  .mockResolvedValue({ inputs: [], outputs: [] });

// Test-visible counters for the mocked TmuxTerminal lifecycle. Used by the
// "WebSocket survives fullscreen toggle" regression test to assert React
// does not remount the terminal subtree when the user toggles fullscreen.
const tmuxMountCount = { current: 0 };
const tmuxUnmountCount = { current: 0 };

vi.mock("../api", () => ({
  fetchPrompt: (...args: unknown[]) => fetchPromptMock(...args),
  fetchNodeIO: (...args: unknown[]) => fetchNodeIOMock(...args),
  markNodeDone: vi.fn(),
  attachSession: vi.fn(),
  artifactUrl: (runId: string, path: string) => `/runs/${runId}/artifact?path=${encodeURIComponent(path)}`,
}));

function MockTmuxTerminal({ session, expanded, onExpand, status }: {
  session: string;
  expanded?: boolean;
  onExpand?: () => void;
  status?: string;
}) {
  useEffect(() => {
    tmuxMountCount.current += 1;
    return () => {
      tmuxUnmountCount.current += 1;
    };
  }, []);
  return (
    <div data-testid="tmux-terminal" data-session={session} data-expanded={expanded} data-status={status}>
      <button data-testid="term-expand" onClick={onExpand}>expand</button>
    </div>
  );
}

vi.mock("./TmuxTerminal", () => ({
  default: MockTmuxTerminal,
}));

vi.mock("./ui/resizable", () => ({
  ResizablePanelGroup: ({
    children,
    ...rest
  }: {
    children: React.ReactNode;
    [key: string]: unknown;
  }) => <div {...rest}>{children}</div>,
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
    tmuxMountCount.current = 0;
    tmuxUnmountCount.current = 0;
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

    it("starts in fullsize when initialTerminalExpanded is true", () => {
      render(
        <TooltipProvider>
          <NodeDetailPanel
            node={makeNode({ status: "running" })}
            runId="run-1"
            initialTerminalExpanded
          />
        </TooltipProvider>,
      );

      const terminal = screen.getByTestId("tmux-terminal");
      expect(terminal.getAttribute("data-expanded")).toBe("true");
      expect(screen.getByTestId("terminal-fullsize")).toBeInTheDocument();
      expect(screen.queryByTestId("details-pane")).not.toBeInTheDocument();
    });

    it("lets the user collapse the terminal even when it started expanded", () => {
      render(
        <TooltipProvider>
          <NodeDetailPanel
            node={makeNode({ status: "running" })}
            runId="run-1"
            initialTerminalExpanded
          />
        </TooltipProvider>,
      );

      expect(screen.getByTestId("terminal-fullsize")).toBeInTheDocument();

      fireEvent.click(screen.getByTestId("term-expand"));

      expect(screen.queryByTestId("terminal-fullsize")).not.toBeInTheDocument();
      expect(screen.getByTestId("details-pane")).toBeInTheDocument();
    });

    // Regression: toggling fullscreen used to swap a `<div>` wrapper for a
    // `<ResizablePanelGroup>` wrapper at the same JSX position, which made
    // React unmount + remount `TmuxTerminal`. The remount tore the WebSocket
    // down, spawned a fresh tmux client, and pushed Claude Code's prompt up
    // by a line on every toggle.
    it("does not remount TmuxTerminal when the user toggles fullscreen", () => {
      render(
        <TooltipProvider>
          <NodeDetailPanel node={makeNode({ status: "running" })} runId="run-1" />
        </TooltipProvider>,
      );

      expect(tmuxMountCount.current).toBe(1);
      expect(tmuxUnmountCount.current).toBe(0);
      const firstNode = screen.getByTestId("tmux-terminal");

      // Expand
      fireEvent.click(screen.getByTestId("term-expand"));
      expect(tmuxMountCount.current).toBe(1);
      expect(tmuxUnmountCount.current).toBe(0);
      expect(screen.getByTestId("tmux-terminal")).toBe(firstNode);

      // Collapse
      fireEvent.click(screen.getByTestId("term-expand"));
      expect(tmuxMountCount.current).toBe(1);
      expect(tmuxUnmountCount.current).toBe(0);
      expect(screen.getByTestId("tmux-terminal")).toBe(firstNode);

      // Expand again
      fireEvent.click(screen.getByTestId("term-expand"));
      expect(tmuxMountCount.current).toBe(1);
      expect(tmuxUnmountCount.current).toBe(0);
      expect(screen.getByTestId("tmux-terminal")).toBe(firstNode);
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

  describe("Image port thumbnails", () => {
    it("shows image thumbnails for image port type", async () => {
      fetchNodeIOMock.mockResolvedValue({
        inputs: [],
        outputs: [
          {
            port: "screenshot",
            repeated: false,
            port_type: "image",
            files: [{ path: "artifacts/node/iter-1/screenshot/capture.png", exists: true, size: 1024, frontmatter: null }],
          },
        ],
      });

      render(
        <TooltipProvider>
          <NodeDetailPanel node={makeNode({ status: "completed" })} runId="run-1" />
        </TooltipProvider>,
      );

      await act(async () => {});
      expect(screen.getByTestId("image-thumbnails")).toBeInTheDocument();
    });

    it("shows port-type badge for image ports", async () => {
      fetchNodeIOMock.mockResolvedValue({
        inputs: [],
        outputs: [
          {
            port: "diagram",
            repeated: false,
            port_type: "image_list",
            files: [{ path: "artifacts/node/iter-1/diagram/a.png", exists: true, size: 512, frontmatter: null }],
          },
        ],
      });

      render(
        <TooltipProvider>
          <NodeDetailPanel node={makeNode({ status: "completed" })} runId="run-1" />
        </TooltipProvider>,
      );

      await act(async () => {});
      expect(screen.getByTestId("port-type-badge")).toHaveTextContent("image_list");
    });

    it("does not show thumbnails for markdown ports", async () => {
      fetchNodeIOMock.mockResolvedValue({
        inputs: [],
        outputs: [
          {
            port: "out",
            repeated: false,
            port_type: "markdown",
            files: [{ path: "artifacts/node/iter-1/out/output.md", exists: true, size: 100, frontmatter: null }],
          },
        ],
      });

      render(
        <TooltipProvider>
          <NodeDetailPanel node={makeNode({ status: "completed" })} runId="run-1" />
        </TooltipProvider>,
      );

      await act(async () => {});
      expect(screen.queryByTestId("image-thumbnails")).not.toBeInTheDocument();
    });
  });

  describe("New statuses (issue #112)", () => {
    it("renders Stopped label in header", () => {
      render(
        <TooltipProvider>
          <NodeDetailPanel
            node={makeNode({ status: "stopped", failure_reason: "user killed it" })}
            runId="run-1"
          />
        </TooltipProvider>,
      );
      expect(screen.getByText("Stopped")).toBeInTheDocument();
      expect(screen.getByText(/user killed it/)).toBeInTheDocument();
    });

    it("renders Stale label in header and stale banner", () => {
      render(
        <TooltipProvider>
          <NodeDetailPanel node={makeNode({ status: "stale" })} runId="run-1" />
        </TooltipProvider>,
      );
      expect(screen.getByText("Stale")).toBeInTheDocument();
      expect(screen.getByText(/agent idle/i)).toBeInTheDocument();
    });

    it("stale node shows Mark complete button", () => {
      render(
        <TooltipProvider>
          <NodeDetailPanel node={makeNode({ status: "stale" })} runId="run-1" />
        </TooltipProvider>,
      );
      expect(screen.getByText("Mark complete")).toBeInTheDocument();
    });

    it("stopped node does not show Mark complete button", () => {
      render(
        <TooltipProvider>
          <NodeDetailPanel node={makeNode({ status: "stopped" })} runId="run-1" />
        </TooltipProvider>,
      );
      expect(screen.queryByText("Mark complete")).not.toBeInTheDocument();
    });

    it("renders terminal for stopped node", () => {
      render(
        <TooltipProvider>
          <NodeDetailPanel node={makeNode({ status: "stopped" })} runId="run-1" />
        </TooltipProvider>,
      );
      expect(screen.getByTestId("tmux-terminal")).toBeInTheDocument();
    });

    it("renders terminal for stale node", () => {
      render(
        <TooltipProvider>
          <NodeDetailPanel node={makeNode({ status: "stale" })} runId="run-1" />
        </TooltipProvider>,
      );
      expect(screen.getByTestId("tmux-terminal")).toBeInTheDocument();
    });
  });
});
