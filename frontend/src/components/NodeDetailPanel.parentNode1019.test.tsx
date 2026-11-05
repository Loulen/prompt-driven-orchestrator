import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// #1019 / ADR-0064 — the I/O | Orchestration tab pair follows the node's REAL
// child runs (any parent node, agent or script), plus the Orchestrator toggle
// which only announces a parent node (tab from the start, empty state).

globalThis.ResizeObserver = class {
  observe() {}
  unobserve() {}
  disconnect() {}
};

vi.mock("../api", () => ({
  fetchPrompt: vi.fn().mockResolvedValue("prompt"),
  fetchNodeIO: vi.fn().mockResolvedValue({ inputs: [], outputs: [] }),
  markNodeDone: vi.fn().mockResolvedValue({ kind: "completed" }),
  releaseNodeCompletion: vi.fn().mockResolvedValue({ kind: "released" }),
  killNode: vi.fn(),
  restartNode: vi.fn(),
  stopNode: vi.fn(),
  retryNode: vi.fn(),
  retryNodePreview: vi.fn().mockResolvedValue({ downstream: [], affected_count: 0, with_artifacts: [] }),
  previewProvisioning: vi.fn().mockResolvedValue({ entries: [], rules: [], conflicts: [] }),
  startNode: vi.fn(),
  attachSession: vi.fn(),
  artifactUrl: (runId: string, path: string) => `/runs/${runId}/artifact?path=${encodeURIComponent(path)}`,
}));

vi.mock("./TmuxTerminal", () => ({
  default: () => <div data-testid="tmux-terminal" />,
}));

vi.mock("./ui/resizable", () => ({
  ResizablePanelGroup: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
  ResizablePanel: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
  ResizableHandle: () => <div />,
}));

vi.mock("./MarkdownArtifactModal", () => ({ default: () => null }));

import NodeDetailPanel from "./NodeDetailPanel";
import type { RunChildEntry, RunChildrenResponse } from "../lib/orchestration";
import type { NodeState } from "../types";

const RUN = "run-parent";

function makeNode(overrides?: Partial<NodeState>): NodeState {
  return {
    node_id: "dispatch",
    status: "running",
    iter: 1,
    started_at: "2026-11-05T14:00:00Z",
    completed_at: null,
    failure_reason: null,
    iterations: [],
    ...overrides,
  };
}

const child = (id: string, over: Partial<RunChildEntry> = {}): RunChildEntry => ({
  run_id: id,
  pipeline_name: "ticket-impl",
  name: id,
  status: "running",
  ...over,
});

/** The `GET /runs/{id}/children` fixture the panel polls; mutable per test. */
let childrenFixture: RunChildrenResponse;
const fetchMock = vi.fn(async (url: string) => {
  if (url === `/runs/${RUN}/children`) {
    return new Response(JSON.stringify(childrenFixture), { status: 200 });
  }
  return new Response("not found", { status: 404 });
});

function withChildren(nodeId: string, children: RunChildEntry[]): RunChildrenResponse {
  return { run_id: RUN, nodes: [{ node_id: nodeId, children }] };
}

beforeEach(() => {
  childrenFixture = { run_id: RUN, nodes: [] };
  fetchMock.mockClear();
  vi.stubGlobal("fetch", fetchMock);
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("NodeDetailPanel — Orchestration tab for any parent node (#1019)", () => {
  it("no toggle + children ⇒ the tab pair shows, I/O stays the default, the tab lists the children", async () => {
    childrenFixture = withChildren("dispatch", [
      child("c1", { status: "completed" }),
      child("c2", { status: "completed" }),
      child("c3"),
    ]);
    render(<NodeDetailPanel node={makeNode()} runId={RUN} />);

    expect(await screen.findByTestId("detail-tabs")).toBeInTheDocument();
    expect(fetchMock).toHaveBeenCalledWith(`/runs/${RUN}/children`);
    expect(screen.getByTestId("detail-tab-io")).toHaveAttribute("data-active", "true");
    expect(screen.getByTestId("tab-child-pills-finished")).toHaveTextContent("2");
    expect(screen.getByTestId("tab-child-pills-running")).toHaveTextContent("1");

    fireEvent.click(screen.getByTestId("detail-tab-orchestration"));
    expect(screen.getAllByTestId("orchestration-child")).toHaveLength(3);
    expect(screen.queryByTestId("orchestration-empty")).toBeNull();
  });

  it("no toggle + no child ⇒ no tab bar, plain I/O pane", async () => {
    childrenFixture = withChildren("other-node", [child("c1")]);
    render(<NodeDetailPanel node={makeNode()} runId={RUN} />);
    await act(async () => {});
    expect(fetchMock).toHaveBeenCalled();
    expect(screen.queryByTestId("detail-tabs")).toBeNull();
    expect(screen.queryByTestId("orchestration-empty")).toBeNull();
  });

  it("toggle + no child ⇒ the tab shows from the start, with the empty state", async () => {
    render(<NodeDetailPanel node={makeNode()} runId={RUN} isOrchestratorNode />);
    await act(async () => {});
    expect(screen.getByTestId("detail-tabs")).toBeInTheDocument();
    fireEvent.click(screen.getByTestId("detail-tab-orchestration"));
    expect(screen.getByTestId("orchestration-empty")).toHaveTextContent("No child run yet.");
  });

  it("a failed child shows the same pills, rows and failure banner as an orchestrator's", async () => {
    childrenFixture = withChildren("dispatch", [child("c1", { status: "failed" }), child("c2", { status: "completed" })]);
    // A failed child parks the node awaiting the user (ADR-0064).
    render(
      <NodeDetailPanel
        node={makeNode({
          status: "awaiting_user",
          awaiting: { cause: "children_pending", message: null, since: "2026-11-05T14:10:00Z" },
        })}
        runId={RUN}
      />,
    );
    fireEvent.click(await screen.findByTestId("detail-tab-orchestration"));
    expect(screen.getByTestId("tab-child-pills-failed")).toHaveTextContent("1");
    expect(screen.getAllByTestId("orchestration-child")).toHaveLength(2);
    expect(screen.getByTestId("orchestration-failed-banner")).toBeInTheDocument();
  });

  it("the first child appearing mid-session mounts the bar silently: the active tab stays I/O", async () => {
    vi.useFakeTimers();
    try {
      render(<NodeDetailPanel node={makeNode()} runId={RUN} />);
      await act(async () => {});
      expect(screen.queryByTestId("detail-tabs")).toBeNull();

      childrenFixture = withChildren("dispatch", [child("c1")]);
      await act(async () => {
        await vi.advanceTimersByTimeAsync(3000);
      });
      expect(screen.getByTestId("detail-tabs")).toBeInTheDocument();
      expect(screen.getByTestId("detail-tab-io")).toHaveAttribute("data-active", "true");
    } finally {
      vi.useRealTimers();
    }
  });

  it("lands on the Orchestration tab when asked (the back arrow from a child), toggle or not", async () => {
    childrenFixture = withChildren("dispatch", [child("c1")]);
    render(<NodeDetailPanel node={makeNode()} runId={RUN} initialDetailTab="orchestration" />);
    expect(await screen.findByTestId("detail-tab-orchestration")).toHaveAttribute("data-active", "true");
    expect(screen.getAllByTestId("orchestration-child")).toHaveLength(1);
  });

  it("the children_pending banner links to the Orchestration tab", async () => {
    childrenFixture = withChildren("dispatch", [child("c1")]);
    render(
      <NodeDetailPanel
        node={makeNode({
          status: "awaiting_user",
          awaiting: { cause: "children_pending", message: null, since: "2026-11-05T14:10:00Z" },
        })}
        runId={RUN}
      />,
    );
    const link = await screen.findByTestId("awaiting-open-orchestration");
    expect(link).toHaveTextContent("Open Orchestration →");
    expect(screen.getByTestId("detail-tab-io")).toHaveAttribute("data-active", "true");
    fireEvent.click(link);
    expect(screen.getByTestId("detail-tab-orchestration")).toHaveAttribute("data-active", "true");
    expect(screen.getAllByTestId("orchestration-child")).toHaveLength(1);
  });
});
