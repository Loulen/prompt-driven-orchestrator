import { render, screen, fireEvent, act } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";

globalThis.ResizeObserver = class {
  observe() {}
  unobserve() {}
  disconnect() {}
};

const fetchPaneMock = vi.fn().mockResolvedValue({ content: "" });
const fetchPromptMock = vi.fn().mockResolvedValue("system prompt here");
const fetchNodeIOMock = vi
  .fn()
  .mockResolvedValue({ inputs: [], outputs: [] });

vi.mock("../api", () => ({
  fetchPane: (...args: unknown[]) => fetchPaneMock(...args),
  fetchPrompt: (...args: unknown[]) => fetchPromptMock(...args),
  fetchNodeIO: (...args: unknown[]) => fetchNodeIOMock(...args),
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
    fetchPaneMock.mockClear();
    fetchPromptMock.mockClear();
    fetchNodeIOMock.mockClear();
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

      // Initial fetches use node.iter (=2)
      await act(async () => {});
      const initialIters = fetchPaneMock.mock.calls.map((c) => c[2]);
      expect(initialIters).toContain(2);

      fetchPaneMock.mockClear();
      fetchNodeIOMock.mockClear();
      fetchPromptMock.mockClear();

      // Open dropdown
      const trigger = screen.getByText(/iter 2/);
      fireEvent.click(trigger);

      // Click iter 1 option
      const option = await screen.findByTestId("iter-option-1");
      fireEvent.click(option);

      // After click, fetches should target iter 1 (regression: was never firing)
      await act(async () => {});
      const newIters = fetchPaneMock.mock.calls.map((c) => c[2]);
      expect(newIters).toContain(1);
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
});
