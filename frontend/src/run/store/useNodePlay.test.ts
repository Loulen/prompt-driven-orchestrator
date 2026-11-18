import { describe, expect, it, vi, beforeEach } from "vitest";
import { act, renderHook, waitFor } from "@testing-library/react";

vi.mock("../../shared/authorization/api", async (importOriginal) => ({
  ...(await importOriginal<object>()),
  askAuthorization: vi.fn(),
}));
vi.mock("../api/nodes", async (importOriginal) => ({
  ...(await importOriginal<object>()),
  startNode: vi.fn().mockResolvedValue({ ok: true, iter: 1 }),
  retryNode: vi.fn().mockResolvedValue({ ok: true, iter: 2, invalidated: [] }),
  retryNodePreview: vi.fn(),
}));

vi.mock("../../shared/ui/toast", () => ({ toast: vi.fn() }));

import { askAuthorization } from "../../shared/authorization/api";
import { toast } from "../../shared/ui/toast";
import { retryNode, retryNodePreview, startNode } from "../api/nodes";
import { useNodePlay, type PlayCandidate } from "./useNodePlay";

const REVIEW: PlayCandidate = { nodeId: "review", kind: "start", status: "pending", nodeName: "Review" };
const BRANCH: PlayCandidate = { nodeId: "branch", kind: "retry", status: "skipped", nodeName: "Branch" };
const CANDIDATES: PlayCandidate[] = [REVIEW, BRANCH];

beforeEach(() => {
  vi.mocked(askAuthorization).mockReset();
  vi.mocked(retryNodePreview).mockReset();
  vi.mocked(startNode).mockClear();
  vi.mocked(retryNode).mockClear();
});

describe("useNodePlay (ADR-0086)", () => {
  it("greys the ▶ the Autorisation d'opération refuses, with its sentence, and nothing before the answer", async () => {
    vi.mocked(askAuthorization).mockResolvedValue([
      { decision: "deny", reason: "reader", sentence: "You can't start nodes in Project P." },
      { decision: "allow" },
    ]);
    const { result } = renderHook(() => useNodePlay("r1", CANDIDATES));
    expect(result.current.canPlay("review", "start")).toBe(true);
    await waitFor(() => expect(result.current.canPlay("review", "start")).toBe(false));
    expect(result.current.whyNotPlay("review", "start")).toBe("You can't start nodes in Project P.");
    expect(result.current.canPlay("branch", "retry")).toBe(true);
    expect(result.current.whyNotPlay("branch", "retry")).toBeUndefined();
    expect(askAuthorization).toHaveBeenCalledWith([
      { resource: "node", verb: { action: "start" }, target: "r1/review" },
      { resource: "node", verb: { action: "retry" }, target: "r1/branch" },
    ]);
  });

  it("starts an on-demand node, and retries a skipped one with nothing downstream at once", async () => {
    vi.mocked(askAuthorization).mockResolvedValue([{ decision: "allow" }, { decision: "allow" }]);
    const { result } = renderHook(() => useNodePlay("r1", CANDIDATES));

    await act(() => result.current.play(REVIEW));
    expect(startNode).toHaveBeenCalledWith("r1", "review");

    vi.mocked(retryNodePreview).mockResolvedValueOnce({ downstream: [], affected_count: 0, with_artifacts: [] });
    await act(() => result.current.play(BRANCH));
    expect(retryNode).toHaveBeenCalledWith("r1", "branch");
    expect(result.current.retryConfirm).toBeNull();
  });

  // CORE-206: the panel's rule, the panel's window.
  it("asks first when the retry resets downstream nodes; cancel sends nothing, confirm retries", async () => {
    vi.mocked(askAuthorization).mockResolvedValue([{ decision: "allow" }, { decision: "allow" }]);
    vi.mocked(retryNodePreview).mockResolvedValue({ downstream: ["end"], affected_count: 1, with_artifacts: ["end"] });
    const { result } = renderHook(() => useNodePlay("r1", CANDIDATES));

    await act(() => result.current.play(BRANCH));
    expect(retryNode).not.toHaveBeenCalled();
    expect(result.current.retryConfirm).toEqual({
      nodeId: "branch",
      nodeName: "Branch",
      confirmation: {
        title: "Retry node Branch?",
        consequences: ["Resets 1 downstream node"],
        danger: true,
        actionLabel: "Retry",
      },
    });
    act(() => result.current.cancelRetry());
    expect(result.current.retryConfirm).toBeNull();
    expect(retryNode).not.toHaveBeenCalled();

    await act(() => result.current.play(BRANCH));
    await act(() => result.current.confirmRetry());
    expect(retryNode).toHaveBeenCalledExactlyOnceWith("r1", "branch");
    expect(result.current.retryConfirm).toBeNull();
  });

  it("asks on a live node too: it ends its live session", async () => {
    vi.mocked(askAuthorization).mockResolvedValue([{ decision: "allow" }]);
    vi.mocked(retryNodePreview).mockResolvedValue({ downstream: [], affected_count: 0, with_artifacts: [] });
    const live: PlayCandidate = { ...BRANCH, status: "running" };
    const candidates = [live];
    const { result } = renderHook(() => useNodePlay("r1", candidates));
    await act(() => result.current.play(live));
    expect(result.current.retryConfirm?.confirmation.consequences).toEqual(["Ends its live session"]);
    expect(retryNode).not.toHaveBeenCalled();
  });

  it("says a refused retry in a toast", async () => {
    vi.mocked(askAuthorization).mockResolvedValue([{ decision: "allow" }, { decision: "allow" }]);
    vi.mocked(retryNodePreview).mockResolvedValue({ downstream: [], affected_count: 0, with_artifacts: [] });
    vi.mocked(retryNode).mockRejectedValueOnce(new Error("resume the run first"));
    const { result } = renderHook(() => useNodePlay("r1", CANDIDATES));
    await act(() => result.current.play(BRANCH));
    expect(toast).toHaveBeenCalledWith("Retry refused — resume the run first", "error");
  });
});
