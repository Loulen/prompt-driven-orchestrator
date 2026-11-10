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

import { askAuthorization } from "../../shared/authorization/api";
import { retryNode, retryNodePreview, startNode } from "../api/nodes";
import { useNodePlay, type PlayCandidate } from "./useNodePlay";

const CANDIDATES: PlayCandidate[] = [
  { nodeId: "review", kind: "start" },
  { nodeId: "branch", kind: "retry" },
];

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

  it("starts an on-demand node and retries a skipped one, asking first when it resets work", async () => {
    vi.mocked(askAuthorization).mockResolvedValue([{ decision: "allow" }, { decision: "allow" }]);
    const { result } = renderHook(() => useNodePlay("r1", CANDIDATES));

    await act(() => result.current.play("review", "start"));
    expect(startNode).toHaveBeenCalledWith("r1", "review");

    vi.mocked(retryNodePreview).mockResolvedValueOnce({ downstream: [], affected_count: 0, with_artifacts: [] });
    await act(() => result.current.play("branch", "retry"));
    expect(retryNode).toHaveBeenCalledWith("r1", "branch");

    vi.mocked(retryNode).mockClear();
    vi.mocked(retryNodePreview).mockResolvedValueOnce({
      downstream: ["end"],
      affected_count: 1,
      with_artifacts: ["end"],
    });
    await act(() => result.current.play("branch", "retry"));
    expect(retryNode).not.toHaveBeenCalled();
    expect(result.current.retryConfirm).toEqual({ nodeId: "branch", affectedCount: 1 });
    await act(() => result.current.confirmRetry());
    expect(retryNode).toHaveBeenCalledWith("r1", "branch");
  });
});
