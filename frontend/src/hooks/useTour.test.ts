/**
 * `readTourAppState` (#987) — « your Run » on a shared instance.
 *
 * The tour used to read the head of the list as the Run it had just caused. With
 * two people playing *First run* on one instance, the head is whoever launched
 * last, so the state now follows the id this browser's Launch received.
 */
import { describe, expect, it } from "vitest";
import { readTourAppState, type TourRunDetailInput, type TourRunEntry } from "./useTour";

const MINE: TourRunEntry = { run_id: "run-mine", name: "my-first-run-k3x9", pipeline_name: "tutorial-interactive", status: "running" };
const THEIRS: TourRunEntry = { run_id: "run-theirs", name: "my-first-run-zzzz", pipeline_name: "tutorial-interactive", status: "running" };

function detail(runId: string): TourRunDetailInput {
  return {
    run_id: runId,
    nodes: { assistant: { node_id: "assistant", status: "running", iter: 1, iterations: [{ iter: 1 }] } },
    node_defs: [{ id: "assistant", name: "assistant", node_type: "agent" }],
  };
}

describe("readTourAppState: your Run", () => {
  it("is the Run this browser launched, even with a newer foreign Run at the head of the list", () => {
    const app = readTourAppState([THEIRS, MINE], null, MINE.run_id);
    expect(app.runs[0].id).toBe(THEIRS.run_id);
    expect(app.launchedRunId).toBe(MINE.run_id);
    expect(app.myRun).toEqual({ id: MINE.run_id, name: MINE.name, nodes: [] });
  });

  it("is nobody's before this browser launched anything", () => {
    const app = readTourAppState([THEIRS, MINE]);
    expect(app.launchedRunId).toBeNull();
    expect(app.myRun).toBeNull();
  });

  it("is known by its id while the list has not caught up with the Launch", () => {
    const app = readTourAppState([THEIRS], null, MINE.run_id);
    expect(app.myRun).toEqual({ id: MINE.run_id, name: MINE.run_id, nodes: [] });
  });

  it("reads its nodes from its own detail only, never from a foreign Run's", () => {
    expect(readTourAppState([THEIRS, MINE], detail(THEIRS.run_id), MINE.run_id).myRun?.nodes).toEqual([]);
    expect(readTourAppState([THEIRS, MINE], detail(MINE.run_id), MINE.run_id).myRun?.nodes).toEqual([
      { id: "assistant", name: "assistant", type: "agent", status: "running", released: false },
    ]);
  });
});
