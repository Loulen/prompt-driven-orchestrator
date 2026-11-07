import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen } from "@testing-library/react";
import { renderHook, act } from "@testing-library/react";
import type { RunState } from "../types";

// The Diff tab (a sibling tab of the panel) fetches on open, but mock the api
// module anyway so the unit test never touches the network.
vi.mock("../api", () => ({
  fetchRunStructuredDiff: vi.fn().mockResolvedValue({ files: [], additions: 0, deletions: 0, files_changed: 0 }),
  // #803: the Info tab reads the Run's drift on mount. Unreadable here, so the
  // Source block never renders and these stay tests about the Stats block.
  fetchSourceDrift: vi.fn().mockRejectedValue(new Error("no drift in this test")),
  fetchRemotes: vi.fn(),
}));

import PipelineInfoPanel from "./PipelineInfoPanel";
import { formatDuration, useRunDuration } from "../lib/runDuration";

function makeRun(overrides: Partial<RunState> = {}): RunState {
  return {
    run_id: "test-run-stats",
    status: "completed",
    pipeline_name: "test-pipe",
    input: "test input",
    started_at: "2026-05-14T00:00:00.000Z",
    completed_at: "2026-05-14T00:01:30.000Z",
    nodes: {},
    edges: [],
    node_defs: [],
    start_node: null,
    end_node: null,
    merge_resolver: null,
    loop_states: {},
    foreach_states: {},
    sessions_spawned: 1234,
    loc: { insertions: 10, deletions: 3, files_changed: 2 },
    cost: { usd: 1.2345, partial: false, unpriced_models: [] },
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
    renderPanel(makeRun());
    expect(screen.queryByText("Sessions")).toBeNull();
    expect(screen.getByText("Node sessions started")).toBeInTheDocument();
  });

  it("renders '—' for Lines changed when loc is null (cleaned run), not '0'", () => {
    renderPanel(makeRun({ status: "archived", loc: null }));
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
    renderPanel(makeRun({ cost: { usd: 0.0525, partial: false, unpriced_models: [] } }));
    expect(screen.getByTestId("stat-cost")).toHaveTextContent("~$0.0525");
  });

  it("marks a partial estimate as a lower bound with a dagger, naming the model (#425)", () => {
    renderPanel(
      makeRun({
        cost: { usd: 2.5, partial: true, unpriced_models: ["claude-sonnet-5"] },
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
    renderPanel(makeRun({ status: "archived", cost: null }));
    const cost = screen.getByTestId("stat-cost");
    expect(cost).toHaveTextContent("—");
    expect(cost).not.toHaveTextContent("$");
  });

  it("renders '—' with a reason (never $0, no dagger) when a harness has no cost source (#553)", () => {
    // A Run with an opencode node: cost is present but not summable → "—" naming
    // the harness in the tooltip, never a misleading $0.
    renderPanel(
      makeRun({
        cost: {
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
            { harness: "claude", usd: 0.342386, form: "derived", partial: false, unpriced_models: [] },
            { harness: "copilot", usd: 0.1005058, form: "reported", partial: false, unpriced_models: [] },
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
            { harness: "claude", usd: 1.5, form: "derived", partial: false, unpriced_models: [] },
          ],
        },
      }),
    );
    expect(screen.getByTestId("stat-cost")).toHaveTextContent("~$1.50 via claude");
  });
});

// pdo-issues#3 (ADR-0081): « Est. cost » of a Run that has child runs is its
// tree cost, always expanded into Parent run / Child runs (N), with a Depth N
// level only when the tree is deeper than one level.
describe("PipelineInfoPanel — tree cost (Run with child runs)", () => {
  const claude = (usd: number, partial = false) => ({
    usd,
    partial,
    unpriced_models: partial ? ["mystery-model"] : [],
    by_harness: [
      {
        harness: "claude",
        usd,
        form: "derived" as const,
        partial,
        unpriced_models: partial ? ["mystery-model"] : [],
      },
    ],
  });

  it("shows the tree cost with Parent run and Child runs (N) on a one-level tree", () => {
    renderPanel(
      makeRun({
        cost: claude(1.84),
        tree_cost: {
          total: claude(6.81),
          parent: claude(1.84),
          children: claude(4.97),
          descendants: 3,
          by_depth: [{ depth: 1, runs: 3, cost: claude(4.97) }],
          unavailable_runs: 0,
        },
      }),
    );
    expect(screen.getByTestId("stat-cost-total")).toHaveTextContent("~$6.81");
    expect(screen.getByTestId("stat-cost-parent")).toHaveTextContent("Parent run");
    expect(screen.getByTestId("stat-cost-parent")).toHaveTextContent("~$1.84");
    expect(screen.getByTestId("stat-cost-children")).toHaveTextContent("Child runs (3)");
    expect(screen.getByTestId("stat-cost-children")).toHaveTextContent("~$4.97");
    // One level only: no Depth breakdown.
    expect(screen.queryByTestId("stat-cost-depths")).toBeNull();
    const title = screen.getByTestId("stat-cost-total").parentElement?.getAttribute("title") ?? "";
    expect(title).toMatch(/estimate/i);
    expect(title).toContain("Includes this run and its 3 child runs.");
    // No decision reference in a visible string.
    expect(screen.getByTestId("stat-cost").textContent).not.toMatch(/ADR|#\d/);
    expect(title).not.toMatch(/ADR|#\d/);
  });

  it("adds a Depth N (k runs) level when the tree is deeper than one level", () => {
    renderPanel(
      makeRun({
        tree_cost: {
          total: claude(6.81),
          parent: claude(1.84),
          children: claude(4.97),
          descendants: 3,
          by_depth: [
            { depth: 1, runs: 2, cost: claude(3.62) },
            { depth: 2, runs: 1, cost: claude(1.35) },
          ],
          unavailable_runs: 0,
        },
      }),
    );
    expect(screen.getByTestId("stat-cost-depth-1")).toHaveTextContent("Depth 1 (2 runs)");
    expect(screen.getByTestId("stat-cost-depth-1")).toHaveTextContent("~$3.62");
    expect(screen.getByTestId("stat-cost-depth-2")).toHaveTextContent("Depth 2 (1 run)");
    expect(screen.getByTestId("stat-cost-depth-2")).toHaveTextContent("~$1.35");
  });

  it("keeps the per-harness ventilation of the tree total", () => {
    const mixed = {
      usd: 6.81,
      partial: false,
      unpriced_models: [],
      by_harness: [
        { harness: "claude", usd: 5.9, form: "derived" as const, partial: false, unpriced_models: [] },
        { harness: "codex", usd: 0.91, form: "derived" as const, partial: false, unpriced_models: [] },
      ],
    };
    renderPanel(
      makeRun({
        tree_cost: {
          total: mixed,
          parent: claude(1.84),
          children: claude(4.97),
          descendants: 1,
          by_depth: [{ depth: 1, runs: 1, cost: claude(4.97) }],
          unavailable_runs: 0,
        },
      }),
    );
    expect(screen.getByTestId("stat-cost-ventilation")).toHaveTextContent(
      "~$5.90 via claude · ~$0.9100 via codex",
    );
  });

  it("puts † on the total and on Child runs when a child is a lower bound", () => {
    renderPanel(
      makeRun({
        tree_cost: {
          total: claude(3.2, true),
          parent: claude(0.95),
          children: claude(2.25, true),
          descendants: 2,
          by_depth: [{ depth: 1, runs: 2, cost: claude(2.25, true) }],
          unavailable_runs: 0,
        },
      }),
    );
    expect(screen.getByTestId("stat-cost-total")).toHaveTextContent("~$3.20†");
    expect(screen.getByTestId("stat-cost-children")).toHaveTextContent("~$2.25†");
    expect(screen.getByTestId("stat-cost-parent")).not.toHaveTextContent("†");
  });

  it("renders '—' on the total when a child has no cost source, Parent run staying readable", () => {
    const uncosted = { usd: 0, partial: false, unpriced_models: [], uncosted_harnesses: ["opencode"] };
    renderPanel(
      makeRun({
        tree_cost: {
          total: { ...uncosted, by_harness: claude(0.95).by_harness },
          parent: claude(0.95),
          children: uncosted,
          descendants: 2,
          by_depth: [{ depth: 1, runs: 2, cost: uncosted }],
          unavailable_runs: 0,
        },
      }),
    );
    expect(screen.getByTestId("stat-cost-total")).toHaveTextContent("—");
    expect(screen.getByTestId("stat-cost-total")).not.toHaveTextContent("$");
    expect(screen.getByTestId("stat-cost-parent")).toHaveTextContent("~$0.9500");
    expect(screen.getByTestId("stat-cost-children")).toHaveTextContent("—");
    const title = screen.getByTestId("stat-cost-total").parentElement?.getAttribute("title") ?? "";
    expect(title).toMatch(/cost unavailable/i);
    expect(title).toContain("opencode");
  });

  it("renders '—' with a reason when a child's cost cannot be computed", () => {
    renderPanel(
      makeRun({
        tree_cost: {
          total: null,
          parent: claude(0.95),
          children: null,
          descendants: 1,
          by_depth: [{ depth: 1, runs: 1, cost: null }],
          unavailable_runs: 1,
        },
      }),
    );
    expect(screen.getByTestId("stat-cost-total")).toHaveTextContent("—");
    expect(screen.getByTestId("stat-cost-parent")).toHaveTextContent("~$0.9500");
    const title = screen.getByTestId("stat-cost-total").parentElement?.getAttribute("title") ?? "";
    expect(title).toMatch(/cannot be computed/i);
  });

  it("leaves a Run without children strictly unchanged (single line)", () => {
    renderPanel(makeRun({ tree_cost: null }));
    expect(screen.getByTestId("stat-cost")).toHaveTextContent("~$1.23");
    expect(screen.queryByTestId("stat-cost-tree")).toBeNull();
    expect(screen.queryByText(/Parent run/)).toBeNull();
  });
});

describe("PipelineInfoPanel — contextual title", () => {
  it("says « Run info » when a Run is selected", () => {
    renderPanel(makeRun());
    expect(screen.getByTestId("info-panel-title")).toHaveTextContent("Run info");
    expect(screen.getByTestId("info-panel-close")).toHaveAttribute("aria-label", "Close run info");
  });

  it("says « Pipeline info » in the editor without a Run", () => {
    renderPanel(null);
    expect(screen.getByTestId("info-panel-title")).toHaveTextContent("Pipeline info");
    expect(screen.getByTestId("info-panel-close")).toHaveAttribute("aria-label", "Close pipeline info");
  });
});
