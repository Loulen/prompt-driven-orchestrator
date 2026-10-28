/**
 * A Run's row, on two lines: status, name, Trigger, child counts
 * and node progress; then pipeline, the extensions' marks, duration and cost.
 * One hover action for its status, then the « ⋯ » menu.
 */
import { render, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { RunListEntry } from "../../../shared/generated/RunListEntry";
import ExtensionContextProvider from "../../../shared/extensions/ExtensionContextProvider";
import { resetExtensions } from "../../../shared/extensions/store";
import { useSelectionStore } from "../../../stores/selectionStore";
import { useGestureRightsStore } from "../../store/gestureRightsStore";
import { NOTHING_REFUSED } from "../../model/runGestures";
import { useRunsPane } from "../../store/runsList/useRunsPane";
import { resetRunCosts } from "../../store/runCosts";
import RunTreeList from "./RunTreeList";

const NOW = Date.now();
const ago = (minutes: number) => new Date(NOW - minutes * 60_000).toISOString();

function entry(over: Partial<RunListEntry>): RunListEntry {
  return {
    run_id: "r1",
    name: "Add a product search",
    pipeline_name: "implement-review",
    status: "completed",
    effective_repo: "/repos/shop",
    stalled: false,
    started_at: ago(30),
    completed_at: ago(16),
    ...over,
  } as RunListEntry;
}

function List({ runs }: { runs: RunListEntry[] }) {
  const pane = useRunsPane({ runs, selectedRunId: null, onSelectRun: vi.fn(), projects: [] });
  return (
    <RunTreeList
      pane={pane}
      runs={runs}
      triggers={[]}
      selectedRunId={null}
      onSelectRun={vi.fn()}
      onNewRun={vi.fn()}
      onShowTriggers={vi.fn()}
    />
  );
}

function mount(runs: RunListEntry[]) {
  return render(
    <ExtensionContextProvider selectedRun={null} runs={runs} projects={[]}>
      <List runs={runs} />
    </ExtensionContextProvider>,
  );
}

const row = (id: string) => document.querySelector(`[data-run-row="${id}"]`) as HTMLElement;

beforeEach(() => {
  resetExtensions();
  resetRunCosts();
  useGestureRightsStore.setState({ rights: NOTHING_REFUSED, viewed: null, listed: new Set() });
  useSelectionStore.getState().clear("runs");
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string) =>
      String(url).includes("/runs/costs")
        ? new Response(
            JSON.stringify({
              runs: [
                {
                  run_id: "r1",
                  cost: { usd: 0.4141, partial: false, unpriced_models: [], uncosted_harnesses: [], by_harness: [] },
                },
              ],
            }),
            { status: 200, headers: { "Content-Type": "application/json" } },
          )
        : new Response("[]", { status: 200, headers: { "Content-Type": "application/json" } }),
    ),
  );
});

describe("a Run's row", () => {
  it("ends its meta line with its duration and its cost", async () => {
    mount([entry({})]);
    expect(within(row("r1")).getByTestId("run-duration")).toHaveTextContent("14m");
    expect(await within(row("r1")).findByTestId("run-cost")).toHaveTextContent("$0.41");
    const meta = within(row("r1")).getByTestId("run-pipeline-name").parentElement!;
    expect(meta).toContainElement(within(row("r1")).getByTestId("run-cost"));
  });

  it("counts a live Run's duration to now, and shows no cost the daemon does not tell", async () => {
    mount([entry({ run_id: "r2", status: "running", completed_at: undefined, started_at: ago(75) })]);
    expect(within(row("r2")).getByTestId("run-duration")).toHaveTextContent("1h15");
    await waitFor(() => expect(fetch).toHaveBeenCalled());
    expect(within(row("r2")).queryByTestId("run-cost")).toBeNull();
  });

  it("draws one square per working node, the rest as a count", () => {
    const nodes = Array.from({ length: 10 }, (_, i) => ({
      id: `n${i}`,
      status: i < 3 ? ("completed" as const) : i === 3 ? ("running" as const) : ("pending" as const),
    }));
    mount([entry({ status: "running", completed_at: undefined, nodes })]);
    const pips = within(row("r1")).getByTestId("run-node-pips");
    expect(pips).toHaveAccessibleName("3 of 10 nodes done");
    expect(pips.querySelectorAll("i")).toHaveLength(8);
    expect(pips.querySelector('i[data-st="running"]')).not.toBeNull();
    expect(pips).toHaveTextContent("+2");
  });

  it("keeps a 200-character name whole in its title, on one line", () => {
    const name = "x".repeat(200);
    mount([entry({ name })]);
    const label = within(row("r1")).getByTestId("run-display-label");
    expect(label).toHaveClass("lp-name");
    expect(label).toHaveAttribute("title", name);
  });

  it("offers the one action its status calls for, then the menu", () => {
    mount([
      entry({ run_id: "a", status: "running", completed_at: undefined }),
      entry({ run_id: "b", status: "paused", completed_at: undefined }),
      entry({ run_id: "c", status: "failed" }),
    ]);
    const bar = (id: string) => within(row(id)).getByTestId("run-row-actions");
    expect(within(bar("a")).getByRole("button", { name: "Pause" })).toBeInTheDocument();
    expect(within(bar("b")).getByRole("button", { name: "Resume" })).toBeInTheDocument();
    expect(within(bar("c")).getByRole("button", { name: "Retry all" })).toBeInTheDocument();
    for (const id of ["a", "b", "c"]) {
      expect(within(bar(id)).getAllByRole("button")).toHaveLength(2);
      expect(within(bar(id)).getByTestId("run-row-menu")).toBeInTheDocument();
    }
  });
});
