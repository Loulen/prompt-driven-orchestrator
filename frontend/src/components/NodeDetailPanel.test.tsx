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
const startNodeMock = vi.fn().mockResolvedValue({ ok: true, iter: 1 });
const retryNodeMock = vi.fn().mockResolvedValue({ ok: true, iter: 2, invalidated: [] });
const retryNodePreviewMock = vi.fn().mockResolvedValue({ downstream: [], affected_count: 0, with_artifacts: [] });
// #490: was `markNodeDone: vi.fn()` inline, which resolves `undefined` — so NO test
// had ever exercised a *Mark complete* click. Made controllable so the verdict
// branches can be driven. Vitest compares arity strictly, hence the spread.
const markNodeDoneMock = vi.fn().mockResolvedValue({ kind: "completed" });

vi.mock("../api", () => ({
  fetchPrompt: (...args: unknown[]) => fetchPromptMock(...args),
  fetchNodeIO: (...args: unknown[]) => fetchNodeIOMock(...args),
  markNodeDone: (...args: unknown[]) => markNodeDoneMock(...args),
  killNode: (...args: unknown[]) => killNodeMock(...args),
  restartNode: (...args: unknown[]) => restartNodeMock(...args),
  stopNode: (...args: unknown[]) => stopNodeMock(...args),
  retryNode: (...args: unknown[]) => retryNodeMock(...args),
  retryNodePreview: (...args: unknown[]) => retryNodePreviewMock(...args),
  startNode: (...args: unknown[]) => startNodeMock(...args),
  attachSession: vi.fn(),
  artifactUrl: (runId: string, path: string) => `/runs/${runId}/artifact?path=${encodeURIComponent(path)}`,
}));

function MockTmuxTerminal({ session, expanded, onExpand, status, paneSource }: {
  session: string;
  expanded?: boolean;
  onExpand?: () => void;
  status?: string;
  paneSource?: { runId: string; nodeId: string; iter: number };
}) {
  useEffect(() => {
    tmuxMountCount.current += 1;
    return () => {
      tmuxUnmountCount.current += 1;
    };
  }, []);
  return (
    <div
      data-testid="tmux-terminal"
      data-session={session}
      data-expanded={expanded}
      data-status={status}
      data-pane-source={paneSource ? JSON.stringify(paneSource) : undefined}
    >
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

  describe("frozen harness (#616)", () => {
    it("shows the node's frozen harness next to the id when present", () => {
      render(<NodeDetailPanel node={makeNode({ harness: "copilot" })} runId="run-1" />);
      const chip = screen.getByTestId("node-frozen-harness");
      expect(chip).toHaveTextContent("copilot");
    });

    it("shows no harness chip when the node never froze one (a pure skip / pre-#616)", () => {
      render(<NodeDetailPanel node={makeNode({ status: "completed", harness: undefined })} runId="run-1" />);
      expect(screen.queryByTestId("node-frozen-harness")).toBeNull();
    });
  });

describe("NodeDetailPanel", () => {
  beforeEach(() => {
    fetchPromptMock.mockClear();
    fetchNodeIOMock.mockClear();
    killNodeMock.mockClear();
    restartNodeMock.mockClear();
    stopNodeMock.mockClear();
    startNodeMock.mockClear();
    retryNodeMock.mockClear();
    retryNodePreviewMock.mockClear();
    retryNodePreviewMock.mockResolvedValue({ downstream: [], affected_count: 0, with_artifacts: [] });
    markNodeDoneMock.mockClear();
    markNodeDoneMock.mockResolvedValue({ kind: "completed" });
    tmuxMountCount.current = 0;
    tmuxUnmountCount.current = 0;
  });

  describe("node cost (#647)", () => {
    it.each([
      ["derived", false, 0.5, "~$0.5000"],
      ["reported", false, 2, "$2.00"],
      ["derived", true, 2, "~$2.00†"],
      [null, false, 3, "~$3.00"],
      ["derived", false, null, "—"],
    ] as const)("renders %s cost beside the end time", (form, partial, usd, text) => {
      render(
        <NodeDetailPanel
          node={makeNode({
            status: "completed",
            completed_at: "2026-01-01T00:01:00Z",
            cost: {
              usd,
              form,
              partial,
              executions: 1,
              readable_executions: usd === null ? 0 : 1,
              unavailable_reasons: usd === null ? ["missing reading"] : [],
            },
          })}
          runId="run-1"
        />,
      );
      const chip = screen.getByTestId("node-cost");
      expect(chip).toHaveTextContent(text);
      expect(chip.parentElement).toHaveTextContent(/ended .* · /);
      if (usd === null) expect(chip).not.toHaveTextContent("$0");
    });

    it("omits the cost chip when the projection has no cost", () => {
      render(<NodeDetailPanel node={makeNode({ cost: undefined })} runId="run-1" />);
      expect(screen.queryByTestId("node-cost")).toBeNull();
    });
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
        "pdo-run-1-test-node-iter-1",
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
        "pdo-run-abc-impl-iter-3",
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

    it("shows the output-validation banner when failed with an output validation reason", () => {
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
        screen.getByTestId("output-validation-banner"),
      ).toBeInTheDocument();
      // #490: the reason VERBATIM. The old hard-coded "after retry" title lied for
      // the `script` fail-fast path, which never retries.
      expect(
        screen.getByTestId("output-validation-banner"),
      ).toHaveTextContent("Failed — output validation failed");
    });

    it("titles the banner with the script fail-fast reason, not \"after retry\"", () => {
      render(
        <TooltipProvider>
          <NodeDetailPanel
            node={makeNode({
              status: "failed",
              failure_reason: "script output validation failed",
            })}
            runId="run-1"
          />
        </TooltipProvider>,
      );
      const banner = screen.getByTestId("output-validation-banner");
      expect(banner).toHaveTextContent("Failed — script output validation failed");
      expect(banner).not.toHaveTextContent("after retry");
      // `includes`, not `startsWith`: the script reason does not start with the
      // after-retry one, so the generic banner must NOT also fire.
      expect(screen.queryByText(/^Failed — script output validation failed$/)).toBeTruthy();
    });

    it("lists the missing output ports of a script fail-fast", () => {
      render(
        <TooltipProvider>
          <NodeDetailPanel
            node={makeNode({
              status: "failed",
              failure_reason: "script output validation failed",
              missing_outputs: ["out"],
            })}
            runId="run-1"
          />
        </TooltipProvider>,
      );
      // Before #490 this list had no home at all in Rust OR TS, so the red banner
      // showed with nothing in it.
      expect(screen.getByTestId("missing-output-list")).toHaveTextContent(
        "Missing outputs: out",
      );
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
        screen.queryByTestId("output-validation-banner"),
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

    // #315: the per-iter rendered prompt lives in the node's working dir, which
    // is destroyed on archive and is not preserved (ADR-0020). So for an
    // archived run the button must not fire an always-404 fetch nor show a
    // stuck "Loading prompt..." — it shows an honest "not preserved" note.
    it("does not fetch the prompt for an archived run", () => {
      render(
        <TooltipProvider>
          <NodeDetailPanel
            node={makeNode({ status: "completed" })}
            runId="run-1"
            isArchived
          />
        </TooltipProvider>,
      );
      expect(fetchPromptMock).not.toHaveBeenCalled();
    });

    it("shows a not-preserved note (not a spinner) when archived", () => {
      render(
        <TooltipProvider>
          <NodeDetailPanel
            node={makeNode({ status: "completed" })}
            runId="run-1"
            isArchived
          />
        </TooltipProvider>,
      );
      fireEvent.click(screen.getByTestId("prompt-toggle"));
      expect(
        screen.getByText("Prompt not preserved for archived runs."),
      ).toBeInTheDocument();
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

    it("does not read the pane itself", () => {
      // The mock for ../api includes no `fetchPane` — if this panel called it, it
      // would throw. The pane is read one layer down, by the terminal that will
      // display it (#617), never polled into a preview here.
      render(
        <TooltipProvider>
          <NodeDetailPanel node={makeNode({ status: "running" })} runId="run-1" />
        </TooltipProvider>,
      );
      // Just verify it renders without error
      expect(screen.getByTestId("tmux-terminal")).toBeInTheDocument();
    });
  });

  // #617: the finished node's frozen pane. The panel's job is to name WHICH
  // iteration is on screen and what state it is in; the terminal decides between
  // attaching and reading the snapshot.
  describe("pane source handed to the terminal", () => {
    it("names the run, the node and the selected iteration", () => {
      render(
        <TooltipProvider>
          <NodeDetailPanel
            node={makeNode({ status: "completed", node_id: "cop", iter: 2 })}
            runId="run-1"
          />
        </TooltipProvider>,
      );
      // A settled node opens minimized (#346) — restore it to mount the terminal.
      fireEvent.click(screen.getByTestId("term-restore"));
      expect(
        screen.getByTestId("tmux-terminal").getAttribute("data-pane-source"),
      ).toBe(JSON.stringify({ runId: "run-1", nodeId: "cop", iter: 2 }));
    });

    it("reports the ITERATION's status, not the node's rollup", async () => {
      // A node running iteration 2 while the selector sits on iteration 1: that
      // older session was reaped long ago. Handing the terminal "running" would
      // send it to attach to a name tmux no longer knows.
      render(
        <TooltipProvider>
          <NodeDetailPanel
            node={makeNode({
              status: "running",
              node_id: "cop",
              iter: 2,
              iterations: [
                { iter: 1, status: "completed", started_at: null, completed_at: null },
                { iter: 2, status: "running", started_at: null, completed_at: null },
              ],
            })}
            runId="run-1"
          />
        </TooltipProvider>,
      );
      await act(async () => {});
      expect(
        screen.getByTestId("tmux-terminal").getAttribute("data-status"),
      ).toBe("running");

      fireEvent.click(screen.getByText(/iter 2/));
      fireEvent.click(await screen.findByTestId("iter-option-1"));
      await act(async () => {});

      const term = screen.getByTestId("tmux-terminal");
      expect(term.getAttribute("data-status")).toBe("completed");
      expect(term.getAttribute("data-pane-source")).toBe(
        JSON.stringify({ runId: "run-1", nodeId: "cop", iter: 1 }),
      );
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

    it("opens the lightbox when a thumbnail is clicked", async () => {
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
      expect(screen.queryByTestId("image-lightbox")).not.toBeInTheDocument();

      fireEvent.click(screen.getByTestId("thumbnail-0"));

      expect(screen.getByTestId("image-lightbox")).toBeInTheDocument();
      expect(screen.getByTestId("lightbox-image").getAttribute("src")).toContain(
        "capture.png",
      );
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

    it("renders Skipped label and greyed banner with the prune reason (#620)", () => {
      render(
        <TooltipProvider>
          <NodeDetailPanel
            node={makeNode({
              status: "skipped",
              skip_reason: "required input never arrived — branch not taken",
            })}
            runId="run-1"
          />
        </TooltipProvider>,
      );
      // The header label distinguishes it from a green "Completed" node…
      expect(screen.getByText("Skipped")).toBeInTheDocument();
      // …and the banner carries *why* it was pruned, at node level.
      expect(
        screen.getByText(/required input never arrived — branch not taken/),
      ).toBeInTheDocument();
    });

    it("skipped banner falls back to a default reason when none is given", () => {
      render(
        <TooltipProvider>
          <NodeDetailPanel node={makeNode({ status: "skipped" })} runId="run-1" />
        </TooltipProvider>,
      );
      expect(screen.getByText(/Skipped — branch not taken/)).toBeInTheDocument();
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

    it("minimizes the terminal for a stopped node", () => {
      // #346: a settled session (stopped) opens minimized — the terminal inset
      // folds to a thin bar and is not mounted, so Outputs take the height.
      render(
        <TooltipProvider>
          <NodeDetailPanel node={makeNode({ status: "stopped" })} runId="run-1" />
        </TooltipProvider>,
      );
      expect(screen.getByTestId("terminal-minimized")).toBeInTheDocument();
      expect(screen.getByTestId("details-pane")).toBeInTheDocument();
      expect(screen.queryByTestId("tmux-terminal")).not.toBeInTheDocument();
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

  // #598 / ADR-0049: an interrupted node (session died on an infra incident) must
  // offer a way back — before this it was a dead end (no Play, no Reopen, no Mark
  // complete), the exact bug reported for an interactive node with a lost session.
  describe("Interrupted node recovery (#598)", () => {
    const interrupted = () =>
      makeNode({ status: "interrupted", failure_reason: "session died — reopen or retry" });

    it("renders the Interrupted label and banner", () => {
      render(
        <TooltipProvider>
          <NodeDetailPanel node={interrupted()} runId="run-1" />
        </TooltipProvider>,
      );
      expect(screen.getByText("Interrupted")).toBeInTheDocument();
      expect(screen.getByTestId("interrupted-banner")).toBeInTheDocument();
      expect(screen.getByText(/session died/i)).toBeInTheDocument();
    });

    it("offers a Play button in the node controls", () => {
      render(
        <TooltipProvider>
          <NodeDetailPanel node={interrupted()} runId="run-1" />
        </TooltipProvider>,
      );
      expect(screen.getByTestId("play-retry-btn")).toHaveTextContent("Play");
    });

    it("Play re-drives the node via retryNode (daemon embeds the reopen)", async () => {
      render(
        <TooltipProvider>
          <NodeDetailPanel node={interrupted()} runId="run-1" />
        </TooltipProvider>,
      );
      await act(async () => {
        fireEvent.click(screen.getByTestId("play-retry-btn"));
      });
      expect(retryNodeMock).toHaveBeenCalledWith("run-1", "test-node");
    });

    it("the banner Reopen button also re-drives the node", async () => {
      render(
        <TooltipProvider>
          <NodeDetailPanel node={interrupted()} runId="run-1" />
        </TooltipProvider>,
      );
      await act(async () => {
        fireEvent.click(screen.getByTestId("interrupted-reopen-btn"));
      });
      expect(retryNodeMock).toHaveBeenCalledWith("run-1", "test-node");
    });

    it("keeps Mark complete reachable (accept the artifacts as they are)", () => {
      render(
        <TooltipProvider>
          <NodeDetailPanel node={interrupted()} runId="run-1" />
        </TooltipProvider>,
      );
      expect(screen.getByTestId("mark-complete-btn")).toBeInTheDocument();
    });

    it("opens minimized — the dead session is never attached", () => {
      render(
        <TooltipProvider>
          <NodeDetailPanel node={interrupted()} runId="run-1" />
        </TooltipProvider>,
      );
      expect(screen.getByTestId("terminal-minimized")).toBeInTheDocument();
      expect(screen.queryByTestId("tmux-terminal")).not.toBeInTheDocument();
    });

    it("hides the recovery affordances for an archived run", () => {
      render(
        <TooltipProvider>
          <NodeDetailPanel node={interrupted()} runId="run-1" isArchived />
        </TooltipProvider>,
      );
      expect(screen.queryByTestId("node-controls")).not.toBeInTheDocument();
      expect(screen.queryByTestId("interrupted-reopen-btn")).not.toBeInTheDocument();
      expect(screen.queryByTestId("mark-complete-btn")).not.toBeInTheDocument();
    });
  });

  describe("Terminated-node default layout (#346)", () => {
    it("defaults a completed node to a minimized terminal with Outputs visible", () => {
      render(
        <TooltipProvider>
          <NodeDetailPanel node={makeNode({ status: "completed" })} runId="run-1" />
        </TooltipProvider>,
      );
      expect(screen.getByTestId("terminal-minimized")).toBeInTheDocument();
      expect(screen.getByTestId("details-pane")).toBeInTheDocument();
      expect(screen.queryByTestId("terminal-fullsize")).not.toBeInTheDocument();
      expect(screen.queryByTestId("tmux-terminal")).not.toBeInTheDocument();
    });

    it.each(["failed", "stopped"] as const)(
      "minimizes the terminal for a %s node",
      (status) => {
        render(
          <TooltipProvider>
            <NodeDetailPanel node={makeNode({ status })} runId="run-1" />
          </TooltipProvider>,
        );
        expect(screen.getByTestId("terminal-minimized")).toBeInTheDocument();
        expect(screen.getByTestId("details-pane")).toBeInTheDocument();
        expect(screen.queryByTestId("tmux-terminal")).not.toBeInTheDocument();
      },
    );

    it("forces minimized for an archived node regardless of a live-ish status", () => {
      // D1: `stale` alone stays split, but `isArchived` overrides — the run's
      // worktree + tmux session are gone.
      render(
        <TooltipProvider>
          <NodeDetailPanel
            node={makeNode({ status: "stale" })}
            runId="run-1"
            isArchived
          />
        </TooltipProvider>,
      );
      expect(screen.getByTestId("terminal-minimized")).toBeInTheDocument();
      expect(screen.queryByTestId("tmux-terminal")).not.toBeInTheDocument();
    });

    it("keeps the terminal in split for a running node", () => {
      render(
        <TooltipProvider>
          <NodeDetailPanel node={makeNode({ status: "running" })} runId="run-1" />
        </TooltipProvider>,
      );
      expect(screen.getByTestId("tmux-terminal")).toBeInTheDocument();
      expect(screen.getByTestId("details-pane")).toBeInTheDocument();
      expect(screen.queryByTestId("terminal-minimized")).not.toBeInTheDocument();
      expect(screen.queryByTestId("terminal-fullsize")).not.toBeInTheDocument();
    });

    it("keeps the terminal visible for an awaiting_user node", () => {
      render(
        <TooltipProvider>
          <NodeDetailPanel node={makeNode({ status: "awaiting_user" })} runId="run-1" />
        </TooltipProvider>,
      );
      expect(screen.getByTestId("tmux-terminal")).toBeInTheDocument();
      expect(screen.getByTestId("details-pane")).toBeInTheDocument();
      expect(screen.queryByTestId("terminal-minimized")).not.toBeInTheDocument();
    });

    it("keeps the terminal visible for a non-archived stale node", () => {
      // D1 lock: `stale` is NOT terminated (session typically still alive,
      // recovery happens inside the terminal).
      render(
        <TooltipProvider>
          <NodeDetailPanel node={makeNode({ status: "stale" })} runId="run-1" />
        </TooltipProvider>,
      );
      expect(screen.getByTestId("tmux-terminal")).toBeInTheDocument();
      expect(screen.getByTestId("details-pane")).toBeInTheDocument();
      expect(screen.queryByTestId("terminal-minimized")).not.toBeInTheDocument();
    });

    it("restores the terminal to split when clicking the minimized bar", () => {
      render(
        <TooltipProvider>
          <NodeDetailPanel node={makeNode({ status: "completed" })} runId="run-1" />
        </TooltipProvider>,
      );
      expect(screen.getByTestId("terminal-minimized")).toBeInTheDocument();

      fireEvent.click(screen.getByTestId("term-restore"));

      expect(screen.getByTestId("tmux-terminal")).toBeInTheDocument();
      expect(screen.getByTestId("details-pane")).toBeInTheDocument();
      expect(screen.queryByTestId("terminal-minimized")).not.toBeInTheDocument();
    });

    it("re-shows the terminal after Retry on a completed node", async () => {
      // affected_count 0 → no confirm dialog → retry fires and terminalView
      // flips to split (session revives).
      retryNodePreviewMock.mockResolvedValue({
        downstream: [],
        affected_count: 0,
        with_artifacts: [],
      });

      render(
        <TooltipProvider>
          <NodeDetailPanel node={makeNode({ status: "completed" })} runId="run-1" />
        </TooltipProvider>,
      );
      expect(screen.getByTestId("terminal-minimized")).toBeInTheDocument();

      await act(async () => {
        fireEvent.click(screen.getByTestId("play-retry-btn"));
      });

      expect(retryNodeMock).toHaveBeenCalledWith("run-1", "test-node");
      expect(screen.queryByTestId("terminal-minimized")).not.toBeInTheDocument();
      expect(screen.getByTestId("tmux-terminal")).toBeInTheDocument();
    });

    it("re-shows the terminal after a confirmed Retry on a completed node", async () => {
      // affected_count > 0 → confirm dialog → confirming fires retry and flips
      // to split (guards handleRetryConfirmed's setTerminalView).
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
      // Still minimized: only the confirm dialog opened, no retry yet.
      expect(screen.getByTestId("terminal-minimized")).toBeInTheDocument();

      await act(async () => {
        fireEvent.click(screen.getByTestId("retry-confirm-ok"));
      });

      expect(retryNodeMock).toHaveBeenCalledWith("run-1", "test-node");
      expect(screen.queryByTestId("terminal-minimized")).not.toBeInTheDocument();
      expect(screen.getByTestId("tmux-terminal")).toBeInTheDocument();
    });

    it("renders the daemon's refusal when Retry is rejected (#487)", async () => {
      // The whole point of the frontend slice: a swallowed 409 rendered as
      // nothing. A refused Retry must show the daemon's "resume the run first".
      retryNodePreviewMock.mockResolvedValue({
        downstream: [],
        affected_count: 0,
        with_artifacts: [],
      });
      retryNodeMock.mockRejectedValueOnce(
        new Error(
          "run r is Failed: no scheduling on a non-running run — resume the run first",
        ),
      );

      render(
        <TooltipProvider>
          <NodeDetailPanel node={makeNode({ status: "failed" })} runId="run-1" />
        </TooltipProvider>,
      );

      expect(screen.queryByTestId("action-verdict")).not.toBeInTheDocument();

      await act(async () => {
        fireEvent.click(screen.getByTestId("play-retry-btn"));
      });

      const verdict = screen.getByTestId("action-verdict");
      expect(verdict).toBeInTheDocument();
      expect(verdict).toHaveAttribute("data-action", "retry");
      expect(verdict).toHaveTextContent("Retry refused");
      expect(verdict).toHaveTextContent("resume the run first");
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
      const failedBanner = failedContainer.querySelector('[data-testid="output-validation-banner"]')
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

    it("shows controls with a disabled Stop and a Start button when node is pending (#204)", () => {
      // Un-gating the controls bar for pending nodes (#204) exposes the Start
      // button. The Stop button stays present but disabled (only `running` can
      // be stopped).
      render(
        <TooltipProvider>
          <NodeDetailPanel node={makeNode({ status: "pending" })} runId="run-1" />
        </TooltipProvider>,
      );
      expect(screen.getByTestId("node-controls")).toBeInTheDocument();
      expect(screen.getByTestId("stop-btn")).toBeDisabled();
      expect(screen.getByTestId("start-btn")).toBeInTheDocument();
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

  describe("Start button (#204)", () => {
    it("shows a Start button on a pending node", () => {
      render(
        <TooltipProvider>
          <NodeDetailPanel node={makeNode({ status: "pending" })} runId="run-1" />
        </TooltipProvider>,
      );
      const startBtn = screen.getByTestId("start-btn");
      expect(startBtn).toBeInTheDocument();
      expect(startBtn).toHaveTextContent("Start");
    });

    it("clicking Start force-spawns the node via startNode API", async () => {
      render(
        <TooltipProvider>
          <NodeDetailPanel node={makeNode({ status: "pending" })} runId="run-1" />
        </TooltipProvider>,
      );
      await act(async () => {
        fireEvent.click(screen.getByTestId("start-btn"));
      });
      expect(startNodeMock).toHaveBeenCalledWith("run-1", "test-node");
    });

    it("does not show a Start button on a running node", () => {
      render(
        <TooltipProvider>
          <NodeDetailPanel node={makeNode({ status: "running" })} runId="run-1" />
        </TooltipProvider>,
      );
      expect(screen.queryByTestId("start-btn")).not.toBeInTheDocument();
    });

    it("does not show a Start button on a pending node of an archived run", () => {
      render(
        <TooltipProvider>
          <NodeDetailPanel
            node={makeNode({ status: "pending" })}
            runId="run-1"
            isArchived
          />
        </TooltipProvider>,
      );
      expect(screen.queryByTestId("start-btn")).not.toBeInTheDocument();
      expect(screen.queryByTestId("node-controls")).not.toBeInTheDocument();
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

  // #490 / ADR-0035 — the refusal of a *Mark complete* click must be visible AT the
  // gesture, and must never blink out. Before this issue `markNodeDone` was mocked
  // as a bare `vi.fn()` resolving `undefined`, so not one of these paths had ever
  // been exercised.
  describe("Mark complete verdict (#490)", () => {
    const awaitingNode = () => makeNode({ status: "awaiting_user" });

    async function clickMarkComplete() {
      await act(async () => {
        fireEvent.click(screen.getByTestId("mark-complete-btn"));
      });
    }

    it("gives the button a testid so a driver need not match the copy", () => {
      render(
        <TooltipProvider>
          <NodeDetailPanel node={awaitingNode()} runId="run-1" />
        </TooltipProvider>,
      );
      expect(screen.getByTestId("mark-complete-btn")).toBeInTheDocument();
    });

    it("shows a recoverable refusal as still-your-turn, with the missing ports", async () => {
      markNodeDoneMock.mockResolvedValue({
        kind: "refused",
        slug: "missing_outputs",
        recoverable: true,
        message: "outputs are incomplete",
        missing: ["review"],
        violations: [],
        body: {},
      });
      render(
        <TooltipProvider>
          <NodeDetailPanel node={awaitingNode()} runId="run-1" />
        </TooltipProvider>,
      );
      await clickMarkComplete();
      const verdict = screen.getByTestId("mark-complete-verdict");
      expect(verdict).toHaveAttribute("data-verdict", "refused");
      expect(verdict).toHaveAttribute("data-slug", "missing_outputs");
      expect(verdict).toHaveAttribute("data-recoverable", "true");
      // The prefix is load-bearing: the gating e2e spec matches /^Missing outputs:/.
      expect(screen.getByTestId("verdict-missing-list")).toHaveTextContent(
        "Missing outputs: review",
      );
      expect(verdict).toHaveTextContent("still your turn");
    });

    it("shows a terminal refusal as the-node-is-now-failed", async () => {
      markNodeDoneMock.mockResolvedValue({
        kind: "refused",
        slug: "frontmatter_retry_exhausted",
        recoverable: false,
        message: "still did not match after the retry",
        missing: [],
        violations: [{ port: "review", field: "verdict", reason: "not in enum" }],
        body: {},
      });
      render(
        <TooltipProvider>
          <NodeDetailPanel node={awaitingNode()} runId="run-1" />
        </TooltipProvider>,
      );
      await clickMarkComplete();
      const verdict = screen.getByTestId("mark-complete-verdict");
      expect(verdict).toHaveAttribute("data-recoverable", "false");
      expect(verdict).toHaveTextContent("now failed");
      expect(screen.getByTestId("verdict-violation-list")).toHaveTextContent(
        "review.verdict",
      );
    });

    it("shows the transition guard's refusal, which used to display nothing at all", async () => {
      markNodeDoneMock.mockResolvedValue({
        kind: "refused",
        slug: "completion_rejected",
        recoverable: false,
        message: "run run-1 is Failed: resume the run first",
        missing: [],
        violations: [],
        body: {},
      });
      render(
        <TooltipProvider>
          <NodeDetailPanel node={makeNode({ status: "failed", failure_reason: "boom" })} runId="run-1" />
        </TooltipProvider>,
      );
      await clickMarkComplete();
      // Pre-#490: read as `missing_outputs` with an empty list, gated on
      // `length > 0`, therefore invisible. This is THE symptom of the issue.
      expect(screen.getByTestId("mark-complete-verdict")).toHaveTextContent(
        "resume the run first",
      );
    });

    it("does not blink the verdict out between two consecutive clicks", async () => {
      markNodeDoneMock.mockResolvedValue({
        kind: "refused",
        slug: "missing_outputs",
        recoverable: true,
        message: "outputs are incomplete",
        missing: ["review"],
        violations: [],
        body: {},
      });
      render(
        <TooltipProvider>
          <NodeDetailPanel node={awaitingNode()} runId="run-1" />
        </TooltipProvider>,
      );
      await clickMarkComplete();
      expect(screen.getByTestId("mark-complete-verdict")).toBeInTheDocument();

      // The second click is the one that matters: the first has nothing to erase.
      // The handler no longer clears before awaiting, so the region always has a
      // tenant — `pending` occupies it and each outcome overwrites it.
      let seenEmpty = false;
      markNodeDoneMock.mockImplementation(async () => {
        seenEmpty = seenEmpty || screen.queryByTestId("mark-complete-verdict") === null;
        return {
          kind: "refused",
          slug: "missing_outputs",
          recoverable: true,
          message: "outputs are incomplete",
          missing: ["review"],
          violations: [],
          body: {},
        };
      });
      await clickMarkComplete();
      expect(seenEmpty).toBe(false);
      expect(screen.getByTestId("mark-complete-verdict")).toBeInTheDocument();
    });

    it("treats a legal duplicate as nothing alarming", async () => {
      markNodeDoneMock.mockResolvedValue({ kind: "noop", reason: "already completed" });
      render(
        <TooltipProvider>
          <NodeDetailPanel node={awaitingNode()} runId="run-1" />
        </TooltipProvider>,
      );
      await clickMarkComplete();
      const verdict = screen.getByTestId("mark-complete-verdict");
      expect(verdict).toHaveAttribute("data-verdict", "noop");
      expect(verdict).toHaveAttribute("data-recoverable", "");
    });

    it("surfaces a transport breakdown instead of swallowing it into console.error", async () => {
      markNodeDoneMock.mockRejectedValue(new Error("Failed to fetch"));
      render(
        <TooltipProvider>
          <NodeDetailPanel node={awaitingNode()} runId="run-1" />
        </TooltipProvider>,
      );
      await clickMarkComplete();
      const verdict = screen.getByTestId("mark-complete-verdict");
      expect(verdict).toHaveAttribute("data-verdict", "error");
      expect(verdict).toHaveTextContent("Failed to fetch");
    });

    it("scopes the verdict to the iteration it was produced for", async () => {
      markNodeDoneMock.mockResolvedValue({
        kind: "refused",
        slug: "missing_outputs",
        recoverable: true,
        message: "outputs are incomplete",
        missing: ["review"],
        violations: [],
        body: {},
      });
      render(
        <TooltipProvider>
          <NodeDetailPanel
            node={makeNode({
              status: "awaiting_user",
              iter: 2,
              iterations: [
                { iter: 1, status: "completed", started_at: null, completed_at: null },
                { iter: 2, status: "awaiting_user", started_at: null, completed_at: null },
              ],
            })}
            runId="run-1"
          />
        </TooltipProvider>,
      );
      await clickMarkComplete();
      expect(screen.getByTestId("mark-complete-verdict")).toBeInTheDocument();

      // Switching iteration must not carry a verdict that belongs to another one.
      fireEvent.click(screen.getByText(/iter 2/));
      const option = await screen.findByTestId("iter-option-1");
      await act(async () => {
        fireEvent.click(option);
      });
      expect(screen.queryByTestId("mark-complete-verdict")).not.toBeInTheDocument();
    });
  });
});
