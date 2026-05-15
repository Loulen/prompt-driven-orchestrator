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

const killNodeMock = vi.fn().mockResolvedValue(undefined);
const restartNodeMock = vi.fn().mockResolvedValue(undefined);
const stopNodeMock = vi.fn().mockResolvedValue(undefined);
const retryNodeMock = vi.fn().mockResolvedValue({ ok: true, iter: 2, invalidated: [] });
const retryNodePreviewMock = vi.fn().mockResolvedValue({ downstream: [], affected_count: 0, with_artifacts: [] });

vi.mock("../api", () => ({
  fetchPrompt: (...args: unknown[]) => fetchPromptMock(...args),
  fetchNodeIO: (...args: unknown[]) => fetchNodeIOMock(...args),
  markNodeDone: vi.fn(),
  killNode: (...args: unknown[]) => killNodeMock(...args),
  restartNode: (...args: unknown[]) => restartNodeMock(...args),
  stopNode: (...args: unknown[]) => stopNodeMock(...args),
  retryNode: (...args: unknown[]) => retryNodeMock(...args),
  retryNodePreview: (...args: unknown[]) => retryNodePreviewMock(...args),
  startNode: vi.fn(),
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
    killNodeMock.mockClear();
    restartNodeMock.mockClear();
    stopNodeMock.mockClear();
    retryNodeMock.mockClear();
    retryNodePreviewMock.mockClear();
    retryNodePreviewMock.mockResolvedValue({ downstream: [], affected_count: 0, with_artifacts: [] });
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

  describe("Stale banner with Stop/Retry (issue #123)", () => {
    it("shows stale banner with idle message", () => {
      render(
        <TooltipProvider>
          <NodeDetailPanel node={makeNode({ status: "stale" })} runId="run-1" />
        </TooltipProvider>,
      );
      const banner = screen.getByTestId("stale-banner");
      expect(banner).toBeInTheDocument();
      expect(banner).toHaveTextContent("Agent idle for >2 min");
      expect(banner).toHaveTextContent("outputs incomplete");
    });

    it("shows Stop and Retry buttons on stale node", () => {
      render(
        <TooltipProvider>
          <NodeDetailPanel node={makeNode({ status: "stale" })} runId="run-1" />
        </TooltipProvider>,
      );
      expect(screen.getByTestId("stale-stop-btn")).toBeInTheDocument();
      expect(screen.getByTestId("stale-retry-btn")).toBeInTheDocument();
    });

    it("calls killNode when Stop is clicked", async () => {
      render(
        <TooltipProvider>
          <NodeDetailPanel node={makeNode({ status: "stale" })} runId="run-1" />
        </TooltipProvider>,
      );
      await act(async () => {
        fireEvent.click(screen.getByTestId("stale-stop-btn"));
      });
      expect(killNodeMock).toHaveBeenCalledWith("run-1", "test-node", 1);
    });

    it("calls restartNode when Retry is clicked", async () => {
      render(
        <TooltipProvider>
          <NodeDetailPanel node={makeNode({ status: "stale" })} runId="run-1" />
        </TooltipProvider>,
      );
      await act(async () => {
        fireEvent.click(screen.getByTestId("stale-retry-btn"));
      });
      expect(restartNodeMock).toHaveBeenCalledWith("run-1", "test-node", 1);
    });

    it("hides Stop/Retry buttons when archived", () => {
      render(
        <TooltipProvider>
          <NodeDetailPanel node={makeNode({ status: "stale" })} runId="run-1" isArchived />
        </TooltipProvider>,
      );
      expect(screen.getByTestId("stale-banner")).toBeInTheDocument();
      expect(screen.queryByTestId("stale-stop-btn")).not.toBeInTheDocument();
      expect(screen.queryByTestId("stale-retry-btn")).not.toBeInTheDocument();
    });

    it("stale indicator is distinct from failed", () => {
      const { container: staleContainer } = render(
        <TooltipProvider>
          <NodeDetailPanel node={makeNode({ status: "stale" })} runId="run-1" />
        </TooltipProvider>,
      );

      const { container: failedContainer } = render(
        <TooltipProvider>
          <NodeDetailPanel node={makeNode({ status: "failed", failure_reason: "test" })} runId="run-2" />
        </TooltipProvider>,
      );

      const staleBanner = staleContainer.querySelector('[data-testid="stale-banner"]');
      const failedBanner = failedContainer.querySelector('[data-testid="frontmatter-exhausted-banner"]')
        ?? failedContainer.querySelector('.border-st-failed\\/30');

      expect(staleBanner).toBeInTheDocument();
      expect(staleBanner?.className).toContain("st-stale");
      if (failedBanner) {
        expect(failedBanner.className).toContain("st-failed");
      }
    });
  });

  describe("Node control buttons", () => {
    it("shows enabled Stop button when node is running", () => {
      render(
        <TooltipProvider>
          <NodeDetailPanel node={makeNode({ status: "running" })} runId="run-1" />
        </TooltipProvider>,
      );
      const stopBtn = screen.getByTestId("stop-btn");
      expect(stopBtn).toBeInTheDocument();
      expect(stopBtn).not.toBeDisabled();
      expect(stopBtn).toHaveTextContent("Stop");
    });

    it("shows disabled Stop button when node is completed", () => {
      render(
        <TooltipProvider>
          <NodeDetailPanel node={makeNode({ status: "completed" })} runId="run-1" />
        </TooltipProvider>,
      );
      const stopBtn = screen.getByTestId("stop-btn");
      expect(stopBtn).toBeDisabled();
    });

    it("shows disabled Stop button when node is failed", () => {
      render(
        <TooltipProvider>
          <NodeDetailPanel node={makeNode({ status: "failed" })} runId="run-1" />
        </TooltipProvider>,
      );
      const stopBtn = screen.getByTestId("stop-btn");
      expect(stopBtn).toBeDisabled();
    });

    it("shows disabled Stop button when node is stopped", () => {
      render(
        <TooltipProvider>
          <NodeDetailPanel node={makeNode({ status: "stopped" })} runId="run-1" />
        </TooltipProvider>,
      );
      const stopBtn = screen.getByTestId("stop-btn");
      expect(stopBtn).toBeDisabled();
    });

    it("does not show controls when node is pending", () => {
      render(
        <TooltipProvider>
          <NodeDetailPanel node={makeNode({ status: "pending" })} runId="run-1" />
        </TooltipProvider>,
      );
      expect(screen.queryByTestId("node-controls")).not.toBeInTheDocument();
    });

    it("does not show controls for archived runs", () => {
      render(
        <TooltipProvider>
          <NodeDetailPanel node={makeNode({ status: "running" })} runId="run-1" isArchived />
        </TooltipProvider>,
      );
      expect(screen.queryByTestId("node-controls")).not.toBeInTheDocument();
    });

    it("shows Retry button with Retry label for running node", () => {
      render(
        <TooltipProvider>
          <NodeDetailPanel node={makeNode({ status: "running" })} runId="run-1" />
        </TooltipProvider>,
      );
      const retryBtn = screen.getByTestId("retry-btn");
      expect(retryBtn).toHaveTextContent("Retry");
    });

    it("shows Play label for failed node", () => {
      render(
        <TooltipProvider>
          <NodeDetailPanel node={makeNode({ status: "failed" })} runId="run-1" />
        </TooltipProvider>,
      );
      const playBtn = screen.getByTestId("play-retry-btn");
      expect(playBtn).toHaveTextContent("Play");
    });

    it("shows Play label for stopped node", () => {
      render(
        <TooltipProvider>
          <NodeDetailPanel node={makeNode({ status: "stopped" })} runId="run-1" />
        </TooltipProvider>,
      );
      const playBtn = screen.getByTestId("play-retry-btn");
      expect(playBtn).toHaveTextContent("Play");
    });

    it("shows Retry label for completed node", () => {
      render(
        <TooltipProvider>
          <NodeDetailPanel node={makeNode({ status: "completed" })} runId="run-1" />
        </TooltipProvider>,
      );
      const playBtn = screen.getByTestId("play-retry-btn");
      expect(playBtn).toHaveTextContent("Retry");
    });

    it("Stop button calls stopNode API", async () => {
      render(
        <TooltipProvider>
          <NodeDetailPanel node={makeNode({ status: "running" })} runId="run-1" />
        </TooltipProvider>,
      );
      await act(async () => {
        fireEvent.click(screen.getByTestId("stop-btn"));
      });
      expect(stopNodeMock).toHaveBeenCalledWith("run-1", "test-node");
    });

    it("Retry button on running node calls retryNode API", async () => {
      render(
        <TooltipProvider>
          <NodeDetailPanel node={makeNode({ status: "running" })} runId="run-1" />
        </TooltipProvider>,
      );
      await act(async () => {
        fireEvent.click(screen.getByTestId("retry-btn"));
      });
      expect(retryNodePreviewMock).toHaveBeenCalledWith("run-1", "test-node");
      expect(retryNodeMock).toHaveBeenCalledWith("run-1", "test-node");
    });

    it("Play button on failed node calls retryNode API", async () => {
      render(
        <TooltipProvider>
          <NodeDetailPanel node={makeNode({ status: "failed" })} runId="run-1" />
        </TooltipProvider>,
      );
      await act(async () => {
        fireEvent.click(screen.getByTestId("play-retry-btn"));
      });
      expect(retryNodePreviewMock).toHaveBeenCalledWith("run-1", "test-node");
      expect(retryNodeMock).toHaveBeenCalledWith("run-1", "test-node");
    });
  });

  describe("Retry confirmation dialog", () => {
    it("shows confirmation dialog when downstream has artifacts", async () => {
      retryNodePreviewMock.mockResolvedValue({
        downstream: ["reviewer"],
        affected_count: 1,
        with_artifacts: ["reviewer"],
      });

      render(
        <TooltipProvider>
          <NodeDetailPanel node={makeNode({ status: "completed" })} runId="run-1" />
        </TooltipProvider>,
      );

      await act(async () => {
        fireEvent.click(screen.getByTestId("play-retry-btn"));
      });

      expect(screen.getByTestId("retry-confirm-backdrop")).toBeInTheDocument();
      expect(screen.getByText(/reset 1 downstream node/)).toBeInTheDocument();
      expect(retryNodeMock).not.toHaveBeenCalled();
    });

    it("shows plural text for multiple downstream nodes", async () => {
      retryNodePreviewMock.mockResolvedValue({
        downstream: ["reviewer", "merger"],
        affected_count: 2,
        with_artifacts: ["reviewer", "merger"],
      });

      render(
        <TooltipProvider>
          <NodeDetailPanel node={makeNode({ status: "completed" })} runId="run-1" />
        </TooltipProvider>,
      );

      await act(async () => {
        fireEvent.click(screen.getByTestId("play-retry-btn"));
      });

      expect(screen.getByText(/reset 2 downstream nodes/)).toBeInTheDocument();
    });

    it("proceeds with retry after confirmation", async () => {
      retryNodePreviewMock.mockResolvedValue({
        downstream: ["reviewer"],
        affected_count: 1,
        with_artifacts: ["reviewer"],
      });

      render(
        <TooltipProvider>
          <NodeDetailPanel node={makeNode({ status: "completed" })} runId="run-1" />
        </TooltipProvider>,
      );

      await act(async () => {
        fireEvent.click(screen.getByTestId("play-retry-btn"));
      });

      expect(retryNodeMock).not.toHaveBeenCalled();

      await act(async () => {
        fireEvent.click(screen.getByTestId("retry-confirm-ok"));
      });

      expect(retryNodeMock).toHaveBeenCalledWith("run-1", "test-node");
      expect(screen.queryByTestId("retry-confirm-backdrop")).not.toBeInTheDocument();
    });

    it("cancels retry when Cancel is clicked", async () => {
      retryNodePreviewMock.mockResolvedValue({
        downstream: ["reviewer"],
        affected_count: 1,
        with_artifacts: ["reviewer"],
      });

      render(
        <TooltipProvider>
          <NodeDetailPanel node={makeNode({ status: "completed" })} runId="run-1" />
        </TooltipProvider>,
      );

      await act(async () => {
        fireEvent.click(screen.getByTestId("play-retry-btn"));
      });

      expect(screen.getByTestId("retry-confirm-backdrop")).toBeInTheDocument();

      fireEvent.click(screen.getByTestId("retry-confirm-cancel"));

      expect(screen.queryByTestId("retry-confirm-backdrop")).not.toBeInTheDocument();
      expect(retryNodeMock).not.toHaveBeenCalled();
    });

    it("skips confirmation when no downstream artifacts", async () => {
      retryNodePreviewMock.mockResolvedValue({
        downstream: ["reviewer"],
        affected_count: 0,
        with_artifacts: [],
      });

      render(
        <TooltipProvider>
          <NodeDetailPanel node={makeNode({ status: "completed" })} runId="run-1" />
        </TooltipProvider>,
      );

      await act(async () => {
        fireEvent.click(screen.getByTestId("play-retry-btn"));
      });

      expect(screen.queryByTestId("retry-confirm-backdrop")).not.toBeInTheDocument();
      expect(retryNodeMock).toHaveBeenCalledWith("run-1", "test-node");
    });

    it("dismisses dialog by clicking backdrop", async () => {
      retryNodePreviewMock.mockResolvedValue({
        downstream: ["reviewer"],
        affected_count: 1,
        with_artifacts: ["reviewer"],
      });

      render(
        <TooltipProvider>
          <NodeDetailPanel node={makeNode({ status: "completed" })} runId="run-1" />
        </TooltipProvider>,
      );

      await act(async () => {
        fireEvent.click(screen.getByTestId("play-retry-btn"));
      });

      fireEvent.click(screen.getByTestId("retry-confirm-backdrop"));

      expect(screen.queryByTestId("retry-confirm-backdrop")).not.toBeInTheDocument();
      expect(retryNodeMock).not.toHaveBeenCalled();
    });
  });
});
