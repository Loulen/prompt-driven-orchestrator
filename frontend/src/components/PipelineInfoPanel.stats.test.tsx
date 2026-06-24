import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen } from "@testing-library/react";
import { renderHook, act } from "@testing-library/react";
import type { RunState } from "../types";

// DiffSection (rendered by the panel) fetches lazily on expand, but mock the api
// module anyway so the unit test never touches the network.
vi.mock("../api", () => ({
  fetchRunDiff: vi.fn().mockResolvedValue(""),
  fetchNodeDiff: vi.fn().mockResolvedValue(""),
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

  it("does not render any cost/token/price field (out of scope)", () => {
    const { container } = renderPanel(makeRun());
    expect(container.textContent ?? "").not.toMatch(/cost|token|price|\$\d/i);
  });
});
