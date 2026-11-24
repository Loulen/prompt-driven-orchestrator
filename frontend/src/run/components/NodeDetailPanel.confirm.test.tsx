/**
 * CORE-206: Stop, Restart clean and the stale node's Stop ask first, in the
 * kit's confirmation window: Cancel, Escape or the backdrop send nothing;
 * Confirm or Enter sends the gesture, as before. A Retry asks there too when
 * it ends a live session or resets downstream nodes, and goes at once
 * otherwise.
 */
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { NodeState } from "../../shared/generated/NodeState";
import { clickBackdrop } from "../../test/clickBackdrop";

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

const { default: NodeDetailPanel } = await import("./NodeDetailPanel");
const { killNode, restartNode } = await import("../api/commands");
const { stopNode, retryNode, retryNodePreview } = await import("../api/nodes");
const { useGestureRightsStore } = await import("../store/gestureRightsStore");
const { NOTHING_REFUSED } = await import("../model/runGestures");

function node(overrides: Partial<NodeState> = {}): NodeState {
  return {
    frontmatter_retries: 0,
    node_id: "implement",
    status: "running",
    iter: 1,
    started_at: "2026-01-01T00:00:00Z",
    completed_at: null,
    failure_reason: null,
    iterations: [],
    ...overrides,
  };
}

const dialog = (name: string) => screen.getByRole("dialog", { name });
const noDialog = () => expect(screen.queryByRole("dialog")).toBeNull();
const click = (el: Element) => act(async () => void fireEvent.click(el));
const preview = (affected: number) =>
  vi.mocked(retryNodePreview).mockReset().mockResolvedValue({ downstream: [], affected_count: affected, with_artifacts: [] });
const lines = (d: HTMLElement) => within(d).getAllByTestId("gesture-confirm-line").map((l) => l.textContent);

beforeEach(() => {
  vi.mocked(stopNode).mockReset().mockResolvedValue(undefined as never);
  vi.mocked(killNode).mockReset().mockResolvedValue(undefined as never);
  vi.mocked(restartNode).mockReset().mockResolvedValue(undefined as never);
  vi.mocked(retryNode).mockReset().mockResolvedValue({ ok: true, iter: 2, invalidated: [] });
  preview(0);
  useGestureRightsStore.setState({ rights: NOTHING_REFUSED, viewed: null, listed: new Set() });
});

describe("the node panel's Stop asks first (CORE-206)", () => {
  it("names the node, says it ends its agent session, in the danger tone, the action focused", async () => {
    render(<NodeDetailPanel node={node()} runId="r1" nodeName="implement" />);
    await click(screen.getByTestId("stop-btn"));
    const d = dialog("Stop node implement?");
    expect(within(d).getByTestId("gesture-confirm-line")).toHaveTextContent("Ends its agent session");
    expect(d.querySelector(".d-confirm__target")).toHaveTextContent("implement");
    const action = within(d).getByRole("button", { name: "Stop" });
    expect(action).toHaveClass("d-btn--danger");
    expect(action).toHaveFocus();
    expect(stopNode).not.toHaveBeenCalled();
  });

  it("its (i) says, in its tooltip, that Retry starts afresh and how to resume by hand", async () => {
    render(<NodeDetailPanel node={node()} runId="r1" nodeName="implement" />);
    await click(screen.getByTestId("stop-btn"));
    const info = within(dialog("Stop node implement?")).getByTestId("gesture-confirm-info");
    expect(info).toHaveAccessibleName("Ends this conversation");
    await userEvent.setup().hover(info);
    const tip = await screen.findByRole("tooltip");
    expect(tip).toHaveTextContent("Ends this conversation");
    expect(tip).toHaveTextContent("Retry starts a fresh agent.");
    expect(tip).toHaveTextContent("(e.g. claude --resume)");
    expect(tip).toHaveTextContent("until the run is archived.");
  });

  it("Escape on the open tooltip closes the tooltip alone, the window stays", async () => {
    const user = userEvent.setup();
    render(<NodeDetailPanel node={node()} runId="r1" nodeName="implement" />);
    await click(screen.getByTestId("stop-btn"));
    const info = within(dialog("Stop node implement?")).getByTestId("gesture-confirm-info");
    info.focus();
    await user.hover(info);
    await screen.findByRole("tooltip");
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("tooltip")).toBeNull();
    expect(dialog("Stop node implement?")).toBeInTheDocument();
    await user.keyboard("{Escape}");
    noDialog();
    expect(stopNode).not.toHaveBeenCalled();
  });

  it("Cancel, Escape or the backdrop send nothing", async () => {
    render(<NodeDetailPanel node={node()} runId="r1" nodeName="implement" />);
    await click(screen.getByTestId("stop-btn"));
    await click(within(dialog("Stop node implement?")).getByRole("button", { name: "Cancel" }));
    noDialog();
    await click(screen.getByTestId("stop-btn"));
    fireEvent.keyDown(dialog("Stop node implement?"), { key: "Escape" });
    noDialog();
    await click(screen.getByTestId("stop-btn"));
    await act(async () => clickBackdrop(dialog("Stop node implement?")));
    noDialog();
    expect(stopNode).not.toHaveBeenCalled();
    expect(screen.getByTestId("stop-btn")).toBeInTheDocument();
  });

  it("Confirm or Enter stops the node", async () => {
    render(<NodeDetailPanel node={node()} runId="r1" nodeName="implement" />);
    await click(screen.getByTestId("stop-btn"));
    await click(within(dialog("Stop node implement?")).getByRole("button", { name: "Stop" }));
    expect(stopNode).toHaveBeenCalledExactlyOnceWith("r1", "implement");
    noDialog();
    await click(screen.getByTestId("stop-btn"));
    // The focus starts on the action: Enter confirms.
    expect(within(dialog("Stop node implement?")).getByRole("button", { name: "Stop" })).toHaveFocus();
    await userEvent.setup().keyboard("{Enter}");
    expect(stopNode).toHaveBeenCalledTimes(2);
    noDialog();
  });

  it("names a node without a name by its id", async () => {
    render(<NodeDetailPanel node={node()} runId="r1" />);
    await click(screen.getByTestId("stop-btn"));
    expect(dialog("Stop node implement?")).toBeInTheDocument();
  });
});

describe("Restart clean asks first (CORE-206)", () => {
  it("asks « Restart node <name>? », cancel sends nothing, confirm restarts the iteration", async () => {
    render(<NodeDetailPanel node={node({ isolated_worktree: true })} runId="r1" nodeName="implement" />);
    await click(screen.getByTestId("restart-iteration-btn"));
    const d = dialog("Restart node implement?");
    expect(within(d).getByTestId("gesture-confirm-line")).toHaveTextContent("Ends its session, restarts clean");
    expect(within(d).queryByTestId("gesture-confirm-info")).toBeNull();
    expect(within(d).getByRole("button", { name: "Restart" })).toHaveClass("d-btn--danger");
    await click(within(d).getByRole("button", { name: "Cancel" }));
    expect(restartNode).not.toHaveBeenCalled();
    await click(screen.getByTestId("restart-iteration-btn"));
    await click(within(dialog("Restart node implement?")).getByRole("button", { name: "Restart" }));
    expect(restartNode).toHaveBeenCalledExactlyOnceWith("r1", "implement", 1);
  });
});

describe("the stale banner's Stop asks first (CORE-206)", () => {
  it("asks « Stop stale node <name>? », cancel sends nothing, confirm marks it failed", async () => {
    render(<NodeDetailPanel node={node({ status: "stale" })} runId="r1" nodeName="implement" />);
    await click(screen.getByTestId("stale-stop-btn"));
    const d = dialog("Stop stale node implement?");
    expect(within(d).getByTestId("gesture-confirm-line")).toHaveTextContent("Marks it failed");
    expect(within(d).getByRole("button", { name: "Stop" })).toHaveClass("d-btn--danger");
    fireEvent.keyDown(d, { key: "Escape" });
    noDialog();
    expect(killNode).not.toHaveBeenCalled();
    await click(screen.getByTestId("stale-stop-btn"));
    await click(within(dialog("Stop stale node implement?")).getByRole("button", { name: "Stop" }));
    expect(killNode).toHaveBeenCalledExactlyOnceWith("r1", "implement", 1);
  });
});

describe("a Retry asks when it ends a live session or resets downstream nodes (CORE-206)", () => {
  it("on a running node: « Retry node <name>? », it ends its live session, in the danger tone; cancel keeps it running", async () => {
    render(<NodeDetailPanel node={node()} runId="r1" nodeName="implement" />);
    await click(screen.getByTestId("retry-btn"));
    const d = dialog("Retry node implement?");
    expect(lines(d)).toEqual(["Ends its live session"]);
    expect(within(d).queryByTestId("gesture-confirm-info")).toBeNull();
    expect(d.querySelector(".d-confirm__target")).toHaveTextContent("implement");
    const action = within(d).getByRole("button", { name: "Retry" });
    expect(action).toHaveClass("d-btn--danger");
    expect(action).toHaveFocus();
    await click(within(d).getByRole("button", { name: "Cancel" }));
    noDialog();
    expect(retryNode).not.toHaveBeenCalled();
  });

  it("both: one line per consequence in one window; Enter retries", async () => {
    preview(2);
    render(<NodeDetailPanel node={node()} runId="r1" nodeName="implement" />);
    await click(screen.getByTestId("retry-btn"));
    expect(lines(dialog("Retry node implement?"))).toEqual(["Ends its live session", "Resets 2 downstream nodes"]);
    await act(async () => void (await userEvent.setup().keyboard("{Enter}")));
    expect(retryNode).toHaveBeenCalledExactlyOnceWith("r1", "implement");
    noDialog();
  });

  it("on a completed node whose downstream holds artifacts: says how many it resets; Escape sends nothing, confirm retries", async () => {
    preview(1);
    render(<NodeDetailPanel node={node({ status: "completed" })} runId="r1" nodeName="implement" />);
    await click(screen.getByTestId("play-retry-btn"));
    const d = dialog("Retry node implement?");
    expect(lines(d)).toEqual(["Resets 1 downstream node"]);
    expect(within(d).getByRole("button", { name: "Retry" })).toHaveClass("d-btn--danger");
    fireEvent.keyDown(d, { key: "Escape" });
    noDialog();
    expect(retryNode).not.toHaveBeenCalled();
    await click(screen.getByTestId("play-retry-btn"));
    await click(within(dialog("Retry node implement?")).getByRole("button", { name: "Retry" }));
    expect(retryNode).toHaveBeenCalledExactlyOnceWith("r1", "implement");
  });

  for (const status of ["failed", "stopped", "completed", "interrupted"] as const) {
    it(`on a ${status} node with nothing downstream: retries at once, no window`, async () => {
      render(<NodeDetailPanel node={node({ status })} runId="r1" nodeName="implement" />);
      await click(screen.getByTestId("play-retry-btn"));
      noDialog();
      expect(retryNode).toHaveBeenCalledExactlyOnceWith("r1", "implement");
    });
  }

  it("the failed banner's Retry follows the same rule", async () => {
    render(<NodeDetailPanel node={node({ status: "failed" })} runId="r1" nodeName="implement" />);
    await click(screen.getByTestId("failed-retry-btn"));
    noDialog();
    expect(retryNode).toHaveBeenCalledTimes(1);
    preview(3);
    await click(screen.getByTestId("failed-retry-btn"));
    expect(lines(dialog("Retry node implement?"))).toEqual(["Resets 3 downstream nodes"]);
    await click(within(dialog("Retry node implement?")).getByRole("button", { name: "Cancel" }));
    expect(retryNode).toHaveBeenCalledTimes(1);
  });

  it("the interrupted banner's Reopen follows the same rule", async () => {
    preview(1);
    render(<NodeDetailPanel node={node({ status: "interrupted" })} runId="r1" nodeName="implement" />);
    await click(screen.getByTestId("interrupted-reopen-btn"));
    await click(within(dialog("Retry node implement?")).getByRole("button", { name: "Retry" }));
    expect(retryNode).toHaveBeenCalledExactlyOnceWith("r1", "implement");
  });

  it("a refused Retry still renders its verdict in the panel", async () => {
    vi.mocked(retryNode).mockRejectedValueOnce(new Error("resume the run first"));
    render(<NodeDetailPanel node={node({ status: "failed" })} runId="r1" nodeName="implement" />);
    await click(screen.getByTestId("play-retry-btn"));
    expect(screen.getByTestId("action-verdict")).toHaveTextContent("resume the run first");
  });
});
