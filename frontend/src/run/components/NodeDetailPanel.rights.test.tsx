/**
 * CORE-42: the view of a Run hides the gestures the caller may not make, as
 * `POST /authorize` answers them, asked once when the view opens (not once
 * per button). Without an extension that restricts, nothing is hidden.
 */
import { act, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Decision, OperationQuestion } from "../../shared/authorization/api";
import type { NodeState } from "../../shared/generated/NodeState";

globalThis.ResizeObserver = class {
  observe() {}
  unobserve() {}
  disconnect() {}
};

vi.mock("../../shared/authorization/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../shared/authorization/api")>()),
  askAuthorization: vi.fn(),
}));
vi.mock("../../settings/api/settings", () => ({
  fetchArtifact: vi.fn(),
  artifactUrl: (runId: string, path: string) => `/runs/${runId}/artifact?path=${path}`,
}));
vi.mock("../../settings/api/agentProfiles", async () => await import("../../settings/api/settings"));
vi.mock("../api/artifacts", async (importOriginal) => ({
  ...(await importOriginal<object>()),
  ...(await import("../../settings/api/settings")),
}));
vi.mock("../api/commands", () => ({ killNode: vi.fn(), restartNode: vi.fn() }));
vi.mock("../api/completion", () => ({ markNodeDone: vi.fn(), releaseNodeCompletion: vi.fn() }));
vi.mock("../api/nodes", () => ({
  fetchPrompt: vi.fn().mockResolvedValue(""),
  fetchNodeIO: vi.fn().mockResolvedValue({ inputs: [], outputs: [] }),
  stopNode: vi.fn(),
  retryNode: vi.fn(),
  retryNodePreview: vi.fn(),
  startNode: vi.fn(),
}));
vi.mock("../../workspace/api/provisioning", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../workspace/api/provisioning")>()),
  previewProvisioning: vi.fn().mockResolvedValue({ entries: [], rules: [], conflicts: [] }),
}));
vi.mock("../../session/components/TmuxTerminal", () => ({
  default: () => <div data-testid="tmux-terminal" />,
}));
vi.mock("../../shared/ui/resizable", () => ({
  ResizablePanelGroup: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
  ResizablePanel: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
  ResizableHandle: () => <div />,
}));

const { askAuthorization } = await import("../../shared/authorization/api");
const { default: NodeDetailPanel } = await import("./NodeDetailPanel");
const { useGestureRightsStore, useRunViewGestures, refreshGestureRights } = await import(
  "../store/gestureRightsStore"
);
const { NOTHING_REFUSED } = await import("../model/runGestures");

const asked = vi.mocked(askAuthorization);

/** Alice, Reader of the Run's Projet: every gesture on a node or the Run refused. */
function answerAsAlice(questions: OperationQuestion[]): Decision[] {
  return questions.map(() => ({ decision: "deny", reason: "Reader of P" }));
}

function answerAllowingAll(questions: OperationQuestion[]): Decision[] {
  return questions.map(() => ({ decision: "allow" }));
}

function node(overrides: Partial<NodeState> = {}): NodeState {
  return {
    frontmatter_retries: 0,
    node_id: "worker",
    status: "running",
    iter: 1,
    started_at: "2026-01-01T00:00:00Z",
    completed_at: null,
    failure_reason: null,
    iterations: [],
    ...overrides,
  };
}

/** The view of the Run `r1`: it asks once, then shows the node's panel. */
function RunView({ state }: { state: NodeState }) {
  useRunViewGestures("r1", ["worker", "end"]);
  return <NodeDetailPanel node={state} runId="r1" />;
}

beforeEach(() => {
  asked.mockReset();
  useGestureRightsStore.setState({ rights: NOTHING_REFUSED, viewed: null, listed: new Set() });
});

describe("the gestures of the view of a Run (CORE-42)", () => {
  it("hides Stop, Retry and Mark complete from a Reader", async () => {
    asked.mockImplementation(async (q) => answerAsAlice(q));
    render(<RunView state={node()} />);
    // Before the answer, everything shows: the daemon still decides.
    expect(screen.getByTestId("stop-btn")).toBeInTheDocument();
    await waitFor(() => expect(screen.queryByTestId("stop-btn")).toBeNull());
    expect(screen.queryByTestId("retry-btn")).toBeNull();
    expect(screen.queryByTestId("mark-complete-btn")).toBeNull();
    expect(screen.queryByTestId("node-controls")).toBeNull();
  });

  it("shows them to a Developer, and hides nothing without an extension", async () => {
    asked.mockImplementation(async (q) => answerAllowingAll(q));
    render(<RunView state={node()} />);
    await waitFor(() => expect(asked).toHaveBeenCalled());
    expect(screen.getByTestId("stop-btn")).toBeInTheDocument();
    expect(screen.getByTestId("retry-btn")).toBeInTheDocument();
    expect(screen.getByTestId("mark-complete-btn")).toBeInTheDocument();
  });

  it("asks once when the view opens, for every gesture of the Run and its nodes, not once per button", async () => {
    asked.mockImplementation(async (q) => answerAllowingAll(q));
    const { rerender } = render(<RunView state={node()} />);
    await waitFor(() => expect(asked).toHaveBeenCalledTimes(1));
    // The Run refreshes (a new node state): no new question.
    rerender(<RunView state={node({ status: "completed" })} />);
    rerender(<RunView state={node({ status: "failed" })} />);
    await act(async () => {});
    expect(asked).toHaveBeenCalledTimes(1);
    const operations = asked.mock.calls[0][0];
    const targets = new Set(operations.map((o) => `${o.resource} ${o.target}`));
    expect(targets).toEqual(new Set(["run r1", "node r1/worker", "node r1/end"]));
    expect(operations).toContainEqual({ resource: "node", verb: { action: "stop" }, target: "r1/worker" });
    expect(operations).toContainEqual({ resource: "node", verb: { action: "mark_node_done" }, target: "r1/worker" });
    expect(operations).toContainEqual({ resource: "run", verb: { action: "retry_all" }, target: "r1" });
  });

  it("follows a change of rights once asked again", async () => {
    asked.mockImplementation(async (q) => answerAsAlice(q));
    render(<RunView state={node()} />);
    await waitFor(() => expect(screen.queryByTestId("stop-btn")).toBeNull());
    asked.mockImplementation(async (q) => answerAllowingAll(q));
    await act(async () => refreshGestureRights());
    await waitFor(() => expect(screen.getByTestId("stop-btn")).toBeInTheDocument());
  });

  it("keeps the last answer when the question fails", async () => {
    asked.mockImplementation(async (q) => answerAsAlice(q));
    render(<RunView state={node()} />);
    await waitFor(() => expect(screen.queryByTestId("stop-btn")).toBeNull());
    asked.mockRejectedValue(new Error("offline"));
    await act(async () => refreshGestureRights());
    expect(screen.queryByTestId("stop-btn")).toBeNull();
  });
});

describe("the answers of the gestures", () => {
  it("drops an answer older than the last question about the same Run", async () => {
    const pending: Array<(d: Decision[]) => void> = [];
    asked.mockImplementation(
      (q) => new Promise<Decision[]>((resolve) => pending.push(() => resolve(answerAllowingAll(q)))),
    );
    render(<RunView state={node()} />);
    await waitFor(() => expect(pending).toHaveLength(1));
    // Asked again (the rights changed): Alice is now a Reader.
    asked.mockImplementation(async (q) => answerAsAlice(q));
    await act(async () => refreshGestureRights());
    await waitFor(() => expect(screen.queryByTestId("stop-btn")).toBeNull());
    // The first, older answer arrives last: it changes nothing.
    await act(async () => pending[0]([]));
    expect(screen.queryByTestId("stop-btn")).toBeNull();
  });
});
