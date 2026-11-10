/**
 * ADR-0064: the child-run counters hang on the card of any parent node, the
 * Orchestrator toggle or not; a node without a child keeps its card.
 */
import { renderHook, waitFor } from "@testing-library/react";
import type { Node } from "@xyflow/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { RunState } from "../../shared/generated/RunState";
import { useCanvasChildRuns } from "./useCanvasChildRuns";

const run = { run_id: "run-1", nodes: {}, node_defs: [] } as unknown as RunState;

function serve() {
  vi.stubGlobal(
    "fetch",
    vi.fn(async () =>
      new Response(
        JSON.stringify({
          run_id: "run-1",
          nodes: [{ node_id: "worker", children: [{ run_id: "c1", status: "running" }, { run_id: "c2", status: "completed" }] }],
        }),
        { status: 200, headers: { "Content-Type": "application/json" } },
      ),
    ),
  );
}

const card = (id: string, data: Record<string, unknown> = {}): Node => ({ id, position: { x: 0, y: 0 }, data });

afterEach(() => vi.unstubAllGlobals());

describe("useCanvasChildRuns", () => {
  it("counts the children of a node without the Orchestrator toggle", async () => {
    serve();
    const { result } = renderHook(() => useCanvasChildRuns(run));
    await waitFor(() =>
      expect((result.current(card("worker")).data as { childCounts?: unknown }).childCounts).toEqual({
        finished: 1,
        failed: 0,
        stale: 0,
        running: 1,
      }),
    );
    expect(typeof (result.current(card("worker")).data as { onOpenOrchestration?: unknown }).onOpenOrchestration).toBe("function");
  });

  it("leaves a node without a child as it was, toggle or not", async () => {
    serve();
    const { result } = renderHook(() => useCanvasChildRuns(run));
    await waitFor(() => expect((result.current(card("worker")).data as { childCounts?: unknown }).childCounts).toBeDefined());
    const lone = card("review", { orchestrator: true });
    expect(result.current(lone)).toBe(lone);
  });
});
