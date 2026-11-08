import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen } from "@testing-library/react";
import { renderHook, act } from "@testing-library/react";
import type { RunState } from "../../shared/generated/RunState";

// The Diff tab (a sibling tab of the panel) fetches on open, but mock the api
// module anyway so the unit test never touches the network.
vi.mock("../../review/api/diff", () => ({
  fetchRunStructuredDiff: vi.fn().mockResolvedValue({ files: [], additions: 0, deletions: 0, files_changed: 0 }),
}));
vi.mock("../../settings/api/settings", () => ({
  // #803: the Info tab reads the Run's drift on mount. Unreadable here, so the
  // Source block never renders and these stay tests about the Stats block.
  fetchSourceDrift: vi.fn().mockRejectedValue(new Error("no drift in this test")),
}));
// The agentic profiles' API answers with the mock of `../../settings/api/settings` too.
vi.mock("../../settings/api/agentProfiles", async () => await import("../../settings/api/settings"));
// The run context's API answers with the mock of `../../settings/api/settings`.
vi.mock("../../run/api/runs", async (importOriginal) => ({
  ...(await importOriginal<object>()),
  ...(await import("../../settings/api/settings")),
}));

vi.mock("../../workspace/api/repos", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../workspace/api/repos")>()),
  fetchRemotes: vi.fn(),
}));

import PipelineInfoPanel from "./PipelineInfoPanel";
import { formatDuration, useRunDuration } from "../../run/model/runDuration";
import { withElements } from "../../test/appearance";

function makeRun(overrides: Partial<RunState> = {}): RunState {
  return {
    collection_states: {},
    switch_states: {},
    sandbox: "off",
    run_id: "test-run-stats",
    status: "completed",
    pipeline_name: "test-pipe",
    input: "test input",
    started_at: "2026-05-14T00:00:00.000Z",
    completed_at: "2026-05-14T00:01:30.000Z",
    nodes: {},
    edges: [],
    node_defs: [],
    start_node: undefined,
    end_node: undefined,
    loop_states: {},
    foreach_states: {},
    sessions_spawned: 1234,
    loc: { insertions: 10, deletions: 3, files_changed: 2 },
    cost: { uncosted_harnesses: [], by_harness: [], usd: 1.2345, partial: false, unpriced_models: [] },
    ...overrides,
  };
}

function renderPanel(run: RunState | null) {
  return render(
    <PipelineInfoPanel
      run={run}
      pipeline={null}
      libraryPipelines={[]}
      onLibraryChanged={() => {}}
      onClose={() => {}}
    />,
  );
}

describe("formatDuration", () => {
  it("renders a compact h/m/s ladder", () => {
    expect(formatDuration(0)).toBe("0s");
    expect(formatDuration(45_000)).toBe("45s");
    expect(formatDuration(4 * 60_000 + 12_000)).toBe("4m 12s");
    expect(formatDuration(60 * 60_000 + 23 * 60_000)).toBe("1h 23m");
  });

  it("returns null for missing/negative/non-finite input", () => {
    expect(formatDuration(null)).toBeNull();
    expect(formatDuration(-1)).toBeNull();
    expect(formatDuration(Infinity)).toBeNull();
  });
});

describe("useRunDuration", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("freezes at completed_at for a terminal run and does not tick", () => {
    const { result } = renderHook(() =>
      useRunDuration("2026-01-01T00:00:00.000Z", "2026-01-01T00:01:30.000Z", "completed"),
    );
    expect(result.current).toBe(90_000);
    act(() => {
      vi.advanceTimersByTime(5_000);
    });
    expect(result.current).toBe(90_000);
  });

  it("ticks every second while the run is live", () => {
    vi.setSystemTime(new Date("2026-01-01T00:00:10.000Z"));
    const { result } = renderHook(() =>
      useRunDuration("2026-01-01T00:00:00.000Z", null, "running"),
    );
    expect(result.current).toBe(10_000);
    act(() => {
      vi.advanceTimersByTime(3_000);
    });
    expect(result.current).toBe(13_000);
  });

  it("keeps ticking through Paused (wall-clock, J3)", () => {
    vi.setSystemTime(new Date("2026-01-01T00:00:05.000Z"));
    const { result } = renderHook(() =>
      useRunDuration("2026-01-01T00:00:00.000Z", null, "paused"),
    );
    expect(result.current).toBe(5_000);
    act(() => {
      vi.advanceTimersByTime(2_000);
    });
    expect(result.current).toBe(7_000);
  });

  it("returns null without a start timestamp", () => {
    const { result } = renderHook(() => useRunDuration(null, null, "running"));
    expect(result.current).toBeNull();
  });
});

describe("PipelineInfoPanel — Stats block (#100)", () => {
  it("renders Duration, Node sessions started and Lines changed", () => {
    withElements({ "run.info.sessions": true });
    renderPanel(makeRun());
    const stats = screen.getByTestId("run-stats");
    expect(stats).toBeInTheDocument();

    // Duration: 90s terminal -> "1m 30s".
    expect(screen.getByTestId("stat-duration")).toHaveTextContent("1m 30s");
    // Sessions: raw cumulative count with a thousands separator.
    expect(screen.getByTestId("stat-sessions")).toHaveTextContent(
      "Node sessions started",
    );
    expect(screen.getByTestId("stat-sessions")).toHaveTextContent("1,234");
    // LOC: +10 / −3 over 2 files.
    const loc = screen.getByTestId("stat-loc");
    expect(loc).toHaveTextContent("+10");
    expect(loc).toHaveTextContent("−3");
    expect(loc).toHaveTextContent("2 files");
  });

  it("labels the sessions stat without the bare word 'Sessions' (no collision)", () => {
    withElements({ "run.info.sessions": true });
    renderPanel(makeRun());
    expect(screen.queryByText("Sessions")).toBeNull();
    expect(screen.getByText("Node sessions started")).toBeInTheDocument();
  });

  it("renders '—' for Lines changed when loc is null (cleaned run), not '0'", () => {
    renderPanel(makeRun({ status: "archived", loc: undefined }));
    const loc = screen.getByTestId("stat-loc");
    expect(loc).toHaveTextContent("—");
    expect(loc).not.toHaveTextContent("+0");
  });

  it("shows a live ticking indicator only for a live run", () => {
    const { rerender } = renderPanel(
      makeRun({ status: "running", completed_at: null }),
    );
    expect(screen.getByTestId("stat-duration-live")).toBeInTheDocument();

    rerender(
      <PipelineInfoPanel
        run={makeRun({ status: "completed" })}
        pipeline={null}
        libraryPipelines={[]}
        onLibraryChanged={() => {}}
        onClose={() => {}}
      />,
    );
    expect(screen.queryByTestId("stat-duration-live")).toBeNull();
  });

  it("does not render the Stats block without a run", () => {
    renderPanel(null);
    expect(screen.queryByTestId("run-stats")).toBeNull();
  });

  it("renders Est. cost as a labelled estimate (#272)", () => {
    renderPanel(makeRun());
    const cost = screen.getByTestId("stat-cost");
    // ≥ $1 → 2 decimals, with a "~$" estimate prefix.
    expect(cost).toHaveTextContent("~$1.23");
    // The row is honestly labelled and the tooltip frames it as an estimate.
    expect(cost).toHaveTextContent("Est. cost");
    const value = cost.querySelector("[title]");
    expect(value?.getAttribute("title")).toMatch(/estimate/i);
    // No lower-bound dagger when the estimate is complete.
    expect(cost).not.toHaveTextContent("†");
  });

  it("uses 4 decimals for sub-dollar estimates", () => {
    renderPanel(makeRun({ cost: { uncosted_harnesses: [], by_harness: [], usd: 0.0525, partial: false, unpriced_models: [] } }));
    expect(screen.getByTestId("stat-cost")).toHaveTextContent("~$0.0525");
  });

  it("marks a partial estimate as a lower bound with a dagger, naming the model (#425)", () => {
    renderPanel(
      makeRun({
        cost: { uncosted_harnesses: [], by_harness: [], usd: 2.5, partial: true, unpriced_models: ["claude-sonnet-5"] },
      }),
    );
    const cost = screen.getByTestId("stat-cost");
    expect(cost).toHaveTextContent("~$2.50");
    expect(cost).toHaveTextContent("†");
    const title = cost.querySelector("[title]")?.getAttribute("title") ?? "";
    expect(title).toMatch(/lower bound/i);
    // The offender is named, not left as an anonymous "an unpriced model".
    expect(title).toContain("claude-sonnet-5");
  });

  it("renders '—' for Est. cost when cost is null (no transcripts), not '$0'", () => {
    renderPanel(makeRun({ status: "archived", cost: undefined }));
    const cost = screen.getByTestId("stat-cost");
    expect(cost).toHaveTextContent("—");
    expect(cost).not.toHaveTextContent("$");
  });

  it("renders '—' with a reason (never $0, no dagger) when a harness has no cost source (#553)", () => {
    // A Run with an opencode node: cost is present but not summable → "—" naming
    // the harness in the tooltip, never a misleading $0.
    renderPanel(
      makeRun({
        cost: { by_harness: [],
          usd: 0,
          partial: false,
          unpriced_models: [],
          uncosted_harnesses: ["opencode"],
        },
      }),
    );
    const cost = screen.getByTestId("stat-cost");
    expect(cost).toHaveTextContent("—");
    expect(cost).not.toHaveTextContent("$");
    expect(cost).not.toHaveTextContent("†");
    const title = cost.querySelector("[title]")?.getAttribute("title") ?? "";
    expect(title).toMatch(/cost unavailable/i);
    expect(title).toContain("opencode");
  });

  it("still ventilates by harness under an unavailable total (#617 FP)", () => {
    // The HP-02 trio: the total goes because of `opencode`, the breakdown stays.
    renderPanel(
      makeRun({
        cost: {
          usd: 0,
          partial: false,
          unpriced_models: [],
          uncosted_harnesses: ["opencode"],
          by_harness: [
            { reported_in_usd: false, harness: "claude", usd: 0.342386, form: "derived", partial: false, unpriced_models: [] },
            { reported_in_usd: false, harness: "copilot", usd: 0.1005058, form: "reported", partial: false, unpriced_models: [] },
          ],
        },
      }),
    );
    const cost = screen.getByTestId("stat-cost");
    expect(cost).toHaveTextContent("—");
    expect(cost).toHaveTextContent("~$0.3424 via claude");
    expect(cost).toHaveTextContent("~$0.1005 via copilot");
    // The breakdown is beside the "—", not standing in for it: no dagger, and the
    // tooltip still says why there is no total.
    expect(cost).not.toHaveTextContent("†");
    const title = cost.querySelector("[title]")?.getAttribute("title") ?? "";
    expect(title).toMatch(/cost unavailable/i);
  });

  it("shows a single slice beside an unavailable total, where a summable one hides it", () => {
    // The ">1 slice" rule exists so a breakdown never repeats the total. When the
    // total is withheld, even one slice says something it cannot.
    renderPanel(
      makeRun({
        cost: {
          usd: 0,
          partial: false,
          unpriced_models: [],
          uncosted_harnesses: ["opencode"],
          by_harness: [
            { reported_in_usd: false, harness: "claude", usd: 1.5, form: "derived", partial: false, unpriced_models: [] },
          ],
        },
      }),
    );
    expect(screen.getByTestId("stat-cost")).toHaveTextContent("~$1.50 via claude");
  });

  describe("a Run with child runs: its tree cost", () => {
    const usd = (amount: number) => ({ usd: amount, partial: false, unpriced_models: [], uncosted_harnesses: [], by_harness: [] });
    const tree = {
      total: usd(4.2),
      parent: usd(1.2345),
      children: usd(2.9655),
      descendants: 4,
      by_depth: [
        { depth: 1, runs: 3, cost: usd(2.1) },
        { depth: 2, runs: 1, cost: usd(0.8655) },
      ],
      unavailable_runs: 0,
    };

    it("shows the tree cost and ⑂ N in Est. cost, broken down in its accessible text", () => {
      renderPanel(makeRun({ tree_cost: tree }));
      const cost = screen.getByTestId("stat-cost");
      expect(screen.getByTestId("stat-cost-total")).toHaveTextContent("~$4.20");
      expect(screen.getByTestId("stat-cost-descendants")).toHaveTextContent("4");
      expect(cost).toHaveTextContent("Parent run ~$1.23");
      expect(cost).toHaveTextContent("Child runs (4) ~$2.97");
      expect(cost).toHaveTextContent("Depth 1 ~$2.10 · Depth 2 ~$0.8655");
      expect(cost).toHaveTextContent("Includes this run and its 4 child runs.");
    });

    it("reads « — » with the count of uncomputable Runs, the known slices kept", () => {
      renderPanel(makeRun({ tree_cost: { ...tree, total: null, children: null, unavailable_runs: 1 } }));
      expect(screen.getByTestId("stat-cost-total")).toHaveTextContent("—");
      // « — » alone: the count goes with an amount.
      expect(screen.queryByTestId("stat-cost-descendants")).toBeNull();
      const cost = screen.getByTestId("stat-cost");
      expect(cost).toHaveTextContent("The cost of 1 run in this tree cannot be computed.");
      expect(cost).toHaveTextContent("Parent run ~$1.23");
    });

    it("marks a lower-bound tree with the amber dagger", () => {
      renderPanel(makeRun({ tree_cost: { ...tree, total: { ...usd(4.2), partial: true, unpriced_models: ["mystery"] } } }));
      expect(screen.getByTestId("stat-cost-total").querySelector(".ei-warn")).toHaveTextContent("†");
    });

    it("leaves a Run without child as it was: no ⑂", () => {
      renderPanel(makeRun());
      expect(screen.queryByTestId("stat-cost-descendants")).toBeNull();
      expect(screen.getByTestId("stat-cost")).toHaveTextContent("~$1.23");
    });
  });
});
