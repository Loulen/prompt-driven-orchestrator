/**
 * A Run's row with a given Apparence (CONTEXT.md « Élément masquable »): an
 * element hidden is not drawn, its status and its name always are; the row
 * takes one line when its whole second line is hidden, on every row alike;
 * the list works the same on one line (select, rename, hover actions, keys).
 */
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { RunListEntry } from "../../../shared/generated/RunListEntry";
import type { ExtensionEntry } from "../../../shared/generated/ExtensionEntry";
import type { UiContribution } from "../../../shared/generated/UiContribution";
import type { TriggerListEntry } from "../../../shared/generated/TriggerListEntry";
import ExtensionContextProvider from "../../../shared/extensions/ExtensionContextProvider";
import { hostReactMajor } from "../../../shared/extensions/shared";
import { resetExtensions, setBundleLoader } from "../../../shared/extensions/store";
import { useSelectionStore } from "../../../stores/selectionStore";
import { useGestureRightsStore } from "../../store/gestureRightsStore";
import { NOTHING_REFUSED } from "../../model/runGestures";
import { useRunsPane } from "../../store/runsList/useRunsPane";
import { resetRunCosts } from "../../store/runCosts";
import { resetAppearance } from "../../../hooks/useAppearance";
import { ELEMENTS_STORAGE_KEY } from "../../../lib/appearance";
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

const RUNS: RunListEntry[] = [
  entry({
    run_id: "parent",
    name: "Review",
    triggered_by: "t1",
    status: "running",
    completed_at: undefined,
    nodes: [
      { id: "a", status: "completed" },
      { id: "b", status: "running" },
    ],
  }),
  entry({ run_id: "child", name: "Slice", parent_run_id: "parent" }),
  entry({ run_id: "lone", name: "Lone", started_at: null, completed_at: undefined, status: "running" }),
];
const TRIGGERS = [{ id: "t1", name: "Nightly" } as TriggerListEntry];

const onSelectRun = vi.fn();

function List({ runs }: { runs: RunListEntry[] }) {
  const pane = useRunsPane({ runs, selectedRunId: null, onSelectRun, projects: [] });
  return (
    <RunTreeList
      pane={pane}
      runs={runs}
      triggers={TRIGGERS}
      selectedRunId={null}
      onSelectRun={onSelectRun}
      onNewRun={vi.fn()}
      onShowTriggers={vi.fn()}
    />
  );
}

function mount(runs: RunListEntry[] = RUNS) {
  return render(
    <ExtensionContextProvider selectedRun={null} runs={runs} projects={[]}>
      <List runs={runs} />
    </ExtensionContextProvider>,
  );
}

/** This browser's copy of the Apparence holds `elements`, read at the next render. */
function appearance(elements: Record<string, boolean>) {
  localStorage.setItem(ELEMENTS_STORAGE_KEY, JSON.stringify(elements));
  resetAppearance();
}

const row = (id: string) => document.querySelector(`[data-run-row="${id}"]`) as HTMLElement;
const rows = () => [...document.querySelectorAll<HTMLElement>("[data-run-row]")];

const EVERY_ELEMENT = {
  "runs.list.child-pills": true,
  "runs.list.node-pips": true,
  "runs.list.parent": true,
  "runs.list.cost": true,
};
const NO_SECOND_LINE = {
  "runs.list.pipeline": false,
  "runs.list.parent": false,
  "runs.list.duration": false,
  "runs.list.cost": false,
};

const EXTENSION: ExtensionEntry = {
  id: "test",
  name: "Test",
  version: "1.0.0",
  ui: {
    bundle_url: "/ext/test/ui.js",
    react_major: hostReactMajor(),
    ui_module_major: null,
    contributions: [
      { location: "run/item", id: "badge", label: "Badge", type: "decoration", component: "Badge" },
      { location: "run/item/people", id: "people", label: "People", type: "decoration", component: "People" },
    ] as UiContribution[],
    commands: [],
    locations: [],
  },
};

function serve(extensions: ExtensionEntry[] = []) {
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string) => {
      const u = String(url);
      const body = u === "/ext" ? extensions : u.includes("/runs/costs") ? { runs: costs() } : u.includes("presences") ? { runs: [] } : [];
      return new Response(JSON.stringify(body), { status: 200, headers: { "Content-Type": "application/json" } });
    }),
  );
}

const costs = () =>
  RUNS.map((r) => ({
    run_id: r.run_id,
    cost: { usd: 0.5, partial: false, unpriced_models: [], uncosted_harnesses: [], by_harness: [] },
  }));

beforeEach(() => {
  localStorage.clear();
  resetAppearance();
  resetExtensions();
  resetRunCosts();
  onSelectRun.mockReset();
  setBundleLoader(async () => ({
    Badge: () => <span data-testid="badge">T-1</span>,
    People: () => <span data-testid="people">AL</span>,
  }));
  useGestureRightsStore.setState({ rights: NOTHING_REFUSED, viewed: null, listed: new Set() });
  useSelectionStore.getState().clear("runs");
  serve();
});

afterEach(() => {
  vi.unstubAllGlobals();
  resetExtensions();
});

describe("a Run's row, with a given Apparence", () => {
  it("draws, by default, its Trigger, its pipeline and its duration, never its child pills, node pips nor cost", async () => {
    mount();
    await waitFor(() => expect(fetch).toHaveBeenCalled());
    const parent = within(row("parent"));
    expect(parent.getByTestId("run-trigger-badge")).toBeInTheDocument();
    expect(parent.getByTestId("run-pipeline-name")).toBeInTheDocument();
    expect(parent.getByTestId("run-duration")).toBeInTheDocument();
    expect(parent.queryByTestId("run-child-pills")).toBeNull();
    expect(parent.queryByTestId("run-node-pips")).toBeNull();
    expect(parent.queryByTestId("run-cost")).toBeNull();
  });

  it("draws every element the Apparence shows", async () => {
    appearance(EVERY_ELEMENT);
    mount();
    const parent = within(row("parent"));
    expect(parent.getByTestId("run-child-pills")).toBeInTheDocument();
    expect(parent.getByTestId("run-node-pips")).toBeInTheDocument();
    expect(await parent.findByTestId("run-cost")).toHaveTextContent("$0.50");
  });

  it.each([
    ["runs.list.trigger", "run-trigger-badge"],
    ["runs.list.child-pills", "run-child-pills"],
    ["runs.list.node-pips", "run-node-pips"],
    ["runs.list.pipeline", "run-pipeline-name"],
    ["runs.list.duration", "run-duration"],
    ["runs.list.cost", "run-cost"],
  ])("leaves %s out once hidden, and keeps the status and the name", async (id, testId) => {
    appearance({ ...EVERY_ELEMENT, [id]: false });
    mount();
    await waitFor(() => expect(fetch).toHaveBeenCalled());
    const parent = within(row("parent"));
    await act(async () => {});
    expect(parent.queryByTestId(testId)).toBeNull();
    expect(parent.getByTestId("run-status-dot")).toBeInTheDocument();
    expect(parent.getByTestId("run-display-label")).toHaveTextContent("Review");
  });

  it("leaves the parent link out once hidden", () => {
    const runs = [entry({ run_id: "p", status: "archived" }), entry({ run_id: "c", parent_run_id: "p" })];
    appearance({ "runs.list.parent": true });
    const { unmount } = mount(runs);
    expect(within(row("c")).getByTestId("run-parent-link")).toBeInTheDocument();
    unmount();
    appearance({});
    mount(runs);
    expect(within(row("c")).queryByTestId("run-parent-link")).toBeNull();
  });

  it("takes one line on every row when its whole second line is hidden, whatever each Run holds", () => {
    appearance(NO_SECOND_LINE);
    mount();
    expect(rows()).toHaveLength(3);
    for (const r of rows()) {
      expect(r).toHaveAttribute("data-lines", "1");
      expect(r.querySelector(".lp-l2")).toBeNull();
      expect(within(r).getByTestId("run-status-dot")).toBeInTheDocument();
      expect(within(r).getByTestId("run-display-label")).toBeInTheDocument();
    }
  });

  it("keeps one height on every row: two lines as soon as one element of the second line shows", () => {
    // A Run never started has no duration: its row keeps the second line all the same.
    appearance({ ...NO_SECOND_LINE, "runs.list.duration": true });
    mount();
    expect(within(row("lone")).queryByTestId("run-duration")).toBeNull();
    expect(new Set(rows().map((r) => r.getAttribute("data-lines")))).toEqual(new Set(["2"]));
  });

  it("keeps the second line while an extension contribution shows there, and takes one line once it is hidden too", async () => {
    serve([EXTENSION]);
    appearance(NO_SECOND_LINE);
    const { unmount } = mount();
    expect(await within(row("parent")).findByTestId("badge")).toBeInTheDocument();
    expect(row("parent")).toHaveAttribute("data-lines", "2");
    unmount();
    appearance({ ...NO_SECOND_LINE, "runs.list.ext.test:badge": false });
    mount();
    await within(row("parent")).findByTestId("people");
    expect(within(row("parent")).queryByTestId("badge")).toBeNull();
    expect(row("parent")).toHaveAttribute("data-lines", "2");
  });

  it("hides each extension contribution on its own, both shown by default", async () => {
    serve([EXTENSION]);
    const { unmount } = mount();
    expect(await within(row("parent")).findByTestId("badge")).toBeInTheDocument();
    expect(within(row("parent")).getByTestId("people")).toBeInTheDocument();
    unmount();
    appearance({ "runs.list.ext.test:people": false });
    mount();
    expect(await within(row("parent")).findByTestId("badge")).toBeInTheDocument();
    expect(within(row("parent")).queryByTestId("people")).toBeNull();
  });
});

describe("the Runs list on one line", () => {
  beforeEach(() => appearance(NO_SECOND_LINE));

  it("selects a Run by a click, by Enter, and by its status mark", () => {
    mount();
    fireEvent.click(row("lone"));
    expect(onSelectRun).toHaveBeenCalledWith("lone");
    fireEvent.keyDown(row("child"), { key: "Enter" });
    expect(onSelectRun).toHaveBeenCalledWith("child");
    fireEvent.click(within(row("parent")).getByTestId("run-select-control"));
    expect(useSelectionStore.getState().runs).toEqual(["parent"]);
  });

  it("keeps its hover actions, and renames on F2", async () => {
    mount();
    const bar = within(row("parent")).getByTestId("run-row-actions");
    expect(within(bar).getByRole("button", { name: "Pause" })).toBeInTheDocument();
    expect(within(bar).getByTestId("run-row-menu")).toBeInTheDocument();
    fireEvent.keyDown(row("lone"), { key: "F2" });
    expect(await screen.findByTestId("rename-input")).toHaveValue("Lone");
  });

  it("folds and unfolds a parent with ← and →", () => {
    mount();
    expect(row("child")).toBeInTheDocument();
    fireEvent.keyDown(row("parent"), { key: "ArrowLeft" });
    expect(row("child")).toBeNull();
    fireEvent.keyDown(row("parent"), { key: "ArrowRight" });
    expect(row("child")).toBeInTheDocument();
  });
});
