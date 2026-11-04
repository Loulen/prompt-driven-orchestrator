import { describe, expect, it } from "vitest";
import type { EditNodeData } from "../../pipeline/components/EditNode";
import type { RunListEntry } from "../../shared/generated/RunListEntry";
import { canvasPreviewRows, SAMPLE_CARDS } from "./runCanvasPreview";
import { mostRecentRun } from "./runsListPreview";

const card = (nodeId: string, over: Partial<EditNodeData> = {}) =>
  ({ nodeId, label: nodeId, nodeType: "agent", inputs: [], outputs: [], interactive: false, status: "completed", ...over }) as EditNodeData;
const names = (rows: EditNodeData[][]) => rows.map((row) => row.map((c) => c.nodeId));

describe("canvasPreviewRows", () => {
  it("draws one card, a fork of two, then one more and one more, Start and End aside", () => {
    const cards = [card("start", { nodeType: "start" }), ...["a", "b", "c", "d", "e", "f"].map((id) => card(id)), card("end", { nodeType: "end" })];
    expect(names(canvasPreviewRows(cards))).toEqual([["a"], ["b", "c"], ["d"], ["e"]]);
  });

  it("puts an orchestrator with child runs at the join, wherever it sits in the pipeline", () => {
    const counts = { running: 1, failed: 0, stale: 0, finished: 2 };
    const cards = [card("orch", { childCounts: counts }), card("a"), card("b"), card("c"), card("d")];
    expect(names(canvasPreviewRows(cards))).toEqual([["a"], ["b", "c"], ["orch"], ["d"]]);
  });

  it("draws a chain when the Run has fewer than three other cards", () => {
    expect(names(canvasPreviewRows([card("a"), card("b")]))).toEqual([["a"], ["b"]]);
    expect(canvasPreviewRows([])).toEqual([]);
  });

  it("draws the sample like the design: plan, tdd and docs, agentic tests, merge", () => {
    expect(names(canvasPreviewRows([...SAMPLE_CARDS]))).toEqual([["plan"], ["tdd", "docs"], ["agentic tests"], ["merge"]]);
  });
});

describe("mostRecentRun", () => {
  const run = (run_id: string, started_at: string | null, status = "completed") => ({ run_id, started_at, status }) as RunListEntry;

  it("is the newest Run, a Run not started yet first, an active one before an archived one", () => {
    expect(mostRecentRun([run("a", "2026-01-01T09:00:00Z"), run("b", "2026-01-01T10:00:00Z")])?.run_id).toBe("b");
    expect(mostRecentRun([run("a", "2026-01-01T09:00:00Z"), run("b", null, "running")])?.run_id).toBe("b");
    expect(mostRecentRun([run("a", "2026-01-01T09:00:00Z"), run("b", "2026-01-01T10:00:00Z", "archived")])?.run_id).toBe("a");
    expect(mostRecentRun([])).toBeNull();
  });
});
