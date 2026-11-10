import { renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { RunState } from "../../shared/generated/RunState";
import type { PaletteSelection } from "./paletteHost";
import { paletteSelection } from "./usePaletteHostSync";
import { MISSING_REFRESH_MS, useSelectionMissingOutputs } from "./useSelectionMissingOutputs";

const fetchNodeIOMock = vi.fn();
vi.mock("../../run/api/nodes", () => ({
  fetchNodeIO: (...args: unknown[]) => fetchNodeIOMock(...args),
}));

const runWith = (iteration: Record<string, unknown>, status = "running") =>
  ({
    run_id: "r1",
    status: "running",
    pipeline_name: "p",
    node_defs: [{ id: "design", name: "design", node_type: "agent" }],
    nodes: {
      design: { node_id: "design", status, iter: 1, iterations: [{ iter: 1, status, started_at: null, completed_at: null, ...iteration }] },
    },
  }) as unknown as RunState;

const select = (run: RunState) =>
  paletteSelection({
    selectedTrigger: null,
    selectedRun: run,
    editTab: { runId: "r1" } as never,
    canvas: { kind: "node", id: "design" },
    pipelines: [],
  });

describe("the selected node, as the command field reads it", () => {
  it("is releasable when its latest iteration is interactive and its session lives", () => {
    const sel = select(runWith({ interactive: true, completion_released: false }));
    expect(sel).toMatchObject({ kind: "node", node: { releasable: true, released: false } });
  });

  it("is not releasable off an interactive iteration, nor once it ended", () => {
    expect(select(runWith({ interactive: false }))).toMatchObject({ node: { releasable: false } });
    expect(select(runWith({ interactive: true }, "failed"))).toMatchObject({ node: { releasable: false } });
  });

  it("says when its Auto-approve is armed", () => {
    expect(select(runWith({ interactive: true, completion_released: true }))).toMatchObject({ node: { released: true } });
  });
});

describe("useSelectionMissingOutputs", () => {
  const node = (status: string, runStatus = "running"): PaletteSelection =>
    ({ kind: "node", run: { id: "r1", status: runStatus }, node: { id: "design", name: null, type: null, status, iter: 3 } }) as PaletteSelection;

  beforeEach(() => {
    fetchNodeIOMock.mockReset();
  });

  it("reads the node's outputs once, null until they arrive, then the missing ports", async () => {
    fetchNodeIOMock.mockResolvedValue({ inputs: [], outputs: [{ port: "out", missing: true }, { port: "final", missing: false }] });
    const { result } = renderHook(() => useSelectionMissingOutputs(node("running")));
    expect(result.current).toBeNull();
    await waitFor(() => expect(result.current).toEqual(["out"]));
    expect(fetchNodeIOMock).toHaveBeenCalledWith("r1", "design", 3);
  });

  it("reads them again while one is missing, until the agent writes it", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      fetchNodeIOMock
        .mockResolvedValueOnce({ inputs: [], outputs: [{ port: "out", missing: true }] })
        .mockResolvedValue({ inputs: [], outputs: [{ port: "out", missing: false }] });
      const { result } = renderHook(() => useSelectionMissingOutputs(node("running")));
      await waitFor(() => expect(result.current).toEqual(["out"]));
      await vi.advanceTimersByTimeAsync(MISSING_REFRESH_MS);
      await waitFor(() => expect(result.current).toEqual([]));
      await vi.advanceTimersByTimeAsync(MISSING_REFRESH_MS * 3);
      // Nothing missing any more: no further read.
      expect(fetchNodeIOMock).toHaveBeenCalledTimes(2);
    } finally {
      vi.useRealTimers();
    }
  });

  it("reads nothing for a node Approve does not apply to, nor for a Run", () => {
    expect(renderHook(() => useSelectionMissingOutputs(node("completed"))).result.current).toEqual([]);
    expect(renderHook(() => useSelectionMissingOutputs(node("running", "paused"))).result.current).toEqual([]);
    expect(renderHook(() => useSelectionMissingOutputs(null)).result.current).toEqual([]);
    expect(fetchNodeIOMock).not.toHaveBeenCalled();
  });

  it("lets the daemon's guard speak when the read fails", async () => {
    let fail: (e: Error) => void = () => {};
    fetchNodeIOMock.mockImplementation(() => new Promise((_, reject) => (fail = reject)));
    const { result } = renderHook(() => useSelectionMissingOutputs(node("running")));
    expect(result.current).toBeNull();
    fail(new Error("down"));
    await waitFor(() => expect(result.current).toEqual([]));
  });
});
