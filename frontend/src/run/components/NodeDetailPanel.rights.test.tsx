/**
 * CORE-42: the view of a Run knows the gestures the caller may not make, as
 * `POST /authorize` answers them, asked once when the view opens (not once
 * per button). CORE-205 (reverses the hiding of CORE-42): a refused gesture
 * stays visible, greyed (`aria-disabled`), its tooltip saying the sentence
 * the extension gave with the refusal. Without an extension that restricts,
 * nothing is greyed.
 */
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
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

const SENTENCE = "You can't change Runs in Project P. Ask an administrator of P.";

/** Alice, Reader of the Run's Projet: every gesture on a node or the Run refused. */
function answerAsAlice(questions: OperationQuestion[]): Decision[] {
  return questions.map(() => ({ decision: "deny", reason: "the test rules refuse this gesture", sentence: SENTENCE }));
}

/** Whether `testId` is on screen, greyed. */
const greyed = (testId: string) => screen.getByTestId(testId).getAttribute("aria-disabled") === "true";

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

describe("the gestures of the view of a Run (CORE-42, CORE-205)", () => {
  it("greys Stop, Retry and Mark complete for a caller who may only read the Run, their tooltip saying the extension's sentence", async () => {
    asked.mockImplementation(async (q) => answerAsAlice(q));
    render(<RunView state={node()} />);
    // Before the answer, nothing is greyed: the daemon still decides.
    expect(greyed("stop-btn")).toBe(false);
    await waitFor(() => expect(greyed("stop-btn")).toBe(true));
    expect(greyed("retry-btn")).toBe(true);
    expect(greyed("mark-complete-btn")).toBe(true);
    expect(screen.getByTestId("node-controls")).toBeInTheDocument();
    // Greyed, not hidden nor `disabled`: still focusable, inert.
    expect(screen.getByTestId("stop-btn")).not.toBeDisabled();
    const { stopNode } = await import("../api/nodes");
    fireEvent.click(screen.getByTestId("stop-btn"));
    expect(stopNode).not.toHaveBeenCalled();
    // The tooltip: the gesture's name, then the sentence; never the technical reason.
    await userEvent.setup().hover(screen.getByTestId("retry-btn"));
    const tip = await screen.findByRole("tooltip");
    expect(tip).toHaveTextContent(`Retry${SENTENCE}`);
    expect(document.body.textContent).not.toContain("the test rules refuse this gesture");
  });

  it("leaves them active for a caller allowed to make them, and greys nothing without an extension", async () => {
    asked.mockImplementation(async (q) => answerAllowingAll(q));
    render(<RunView state={node()} />);
    await waitFor(() => expect(asked).toHaveBeenCalled());
    expect(greyed("stop-btn")).toBe(false);
    expect(greyed("retry-btn")).toBe(false);
    expect(greyed("mark-complete-btn")).toBe(false);
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
    await waitFor(() => expect(greyed("stop-btn")).toBe(true));
    asked.mockImplementation(async (q) => answerAllowingAll(q));
    await act(async () => refreshGestureRights());
    await waitFor(() => expect(greyed("stop-btn")).toBe(false));
  });

  it("keeps the last answer when the question fails", async () => {
    asked.mockImplementation(async (q) => answerAsAlice(q));
    render(<RunView state={node()} />);
    await waitFor(() => expect(greyed("stop-btn")).toBe(true));
    asked.mockRejectedValue(new Error("offline"));
    await act(async () => refreshGestureRights());
    expect(greyed("stop-btn")).toBe(true);
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
    await waitFor(() => expect(greyed("stop-btn")).toBe(true));
    // The first, older answer arrives last: it changes nothing.
    await act(async () => pending[0]([]));
    expect(greyed("stop-btn")).toBe(true);
  });
});
