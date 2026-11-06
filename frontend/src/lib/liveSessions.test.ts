import { describe, it, expect, vi } from "vitest";
import {
  collectLiveSessions,
  liveSessionsOfRun,
  paneIterStatus,
  searchSessions,
} from "./liveSessions";
import type { NodeState, RunListEntry, RunState } from "../types";

function nodeState(node_id: string, status: NodeState["status"], iter = 1, over?: Partial<NodeState>): NodeState {
  return {
    node_id,
    status,
    iter,
    started_at: null,
    completed_at: null,
    failure_reason: null,
    iterations: [],
    ...over,
  };
}

function run(run_id: string, over?: Partial<RunState>): RunState {
  return {
    run_id,
    status: "running",
    pipeline_name: "p",
    name: `Run ${run_id}`,
    input: null,
    started_at: null,
    completed_at: null,
    nodes: {},
    edges: [],
    node_defs: [],
    start_node: null,
    end_node: null,
    merge_resolver: null,
    ...over,
  } as RunState;
}

function def(id: string, name?: string) {
  return { id, name, node_type: "agent", view_x: null, view_y: null, inputs: [], outputs: [] } as unknown as RunState["node_defs"][number];
}

describe("live sessions of a Run (#1040)", () => {
  it("lists the Manager, then the running and awaiting nodes in graph order, by name", () => {
    const r = run("r1", {
      has_manager: true,
      node_defs: [def("design", "Design"), def("implement"), def("review", "Review")],
      nodes: {
        review: nodeState("review", "awaiting_user", 2),
        design: nodeState("design", "running", 3),
        implement: nodeState("implement", "completed"),
      },
    });
    expect(liveSessionsOfRun(r)).toEqual([
      { session: "pdo-mgr-r1", kind: "manager", runId: "r1", runName: "Run r1", subject: "Manager" },
      {
        session: "pdo-r1-design-iter-3",
        kind: "node",
        runId: "r1",
        runName: "Run r1",
        subject: "Design",
        paneSource: { runId: "r1", nodeId: "design", iter: 3 },
      },
      {
        session: "pdo-r1-review-iter-2",
        kind: "node",
        runId: "r1",
        runName: "Run r1",
        subject: "Review",
        paneSource: { runId: "r1", nodeId: "review", iter: 2 },
      },
    ]);
  });

  it("lists no Manager the daemon did not find, and names a nameless node by its id", () => {
    const r = run("r1", { has_manager: false, name: "  ", nodes: { x: nodeState("x", "running") } });
    expect(liveSessionsOfRun(r)).toEqual([
      expect.objectContaining({ session: "pdo-r1-x-iter-1", subject: "x", runName: null }),
    ]);
  });

  it("reads only the live Runs, and leaves out one whose detail fails", async () => {
    const entries = [
      { run_id: "a", status: "running" },
      { run_id: "b", status: "completed" },
      { run_id: "c", status: "awaiting_user" },
      { run_id: "d", status: "paused" },
    ] as RunListEntry[];
    const fetchRun = vi.fn(async (id: string) => {
      if (id === "c") throw new Error("boom");
      return run(id, { has_manager: true });
    });
    const sessions = await collectLiveSessions({ fetchRuns: async () => entries, fetchRun });
    expect(fetchRun.mock.calls.map(([id]) => id)).toEqual(["a", "c", "d"]);
    expect(sessions.map((s) => s.session)).toEqual(["pdo-mgr-a", "pdo-mgr-d"]);
  });

  it("fails when the Runs cannot be listed", async () => {
    await expect(
      collectLiveSessions({ fetchRuns: () => Promise.reject(new Error("down")), fetchRun: vi.fn() }),
    ).rejects.toThrow("down");
  });

  it("searches by Run name and node name, every word, ignoring case and accents", () => {
    const refs = [
      ...liveSessionsOfRun(
        run("r1", {
          name: "US #5 - Sécurité",
          has_manager: true,
          node_defs: [def("grill", "Grill")],
          nodes: { grill: nodeState("grill", "running") },
        }),
      ),
      ...liveSessionsOfRun(
        run("r2", { name: "Issue 1040", node_defs: [def("impl", "Implement")], nodes: { impl: nodeState("impl", "running") } }),
      ),
    ];
    expect(searchSessions(refs, "").length).toBe(3);
    expect(searchSessions(refs, "securite").map((r) => r.subject)).toEqual(["Manager", "Grill"]);
    expect(searchSessions(refs, "sécurité GRILL").map((r) => r.session)).toEqual(["pdo-r1-grill-iter-1"]);
    expect(searchSessions(refs, "implement").map((r) => r.runId)).toEqual(["r2"]);
    expect(searchSessions(refs, "manager 1040")).toEqual([]);
  });

  it("reads the status of the iteration a pane shows", () => {
    const r = run("r1", {
      nodes: {
        design: nodeState("design", "running", 2, {
          iterations: [
            { iter: 1, status: "failed", started_at: null, completed_at: null },
            { iter: 2, status: "running", started_at: null, completed_at: null },
          ],
        }),
        bare: nodeState("bare", "completed", 1),
      },
    });
    expect(paneIterStatus(r, "design", 1)).toBe("failed");
    expect(paneIterStatus(r, "design", 2)).toBe("running");
    expect(paneIterStatus(r, "bare", 1)).toBe("completed");
    expect(paneIterStatus(r, "bare", 4)).toBeUndefined();
    expect(paneIterStatus(r, "ghost", 1)).toBeUndefined();
  });
});
