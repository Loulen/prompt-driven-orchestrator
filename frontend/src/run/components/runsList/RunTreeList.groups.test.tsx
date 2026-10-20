/**
 * The Projet groups of the Runs list fold by their header's chevron. The fold
 * is kept per browser (local storage): it survives a reload and a new tab,
 * which mount the list afresh. A folded group keeps its count and says how
 * many of its Runs await the user.
 */
import { fireEvent, render, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { RunListEntry } from "../../../shared/generated/RunListEntry";
import type { Project } from "../../../workspace/model/project";
import ExtensionContextProvider from "../../../shared/extensions/ExtensionContextProvider";
import { resetExtensions } from "../../../shared/extensions/store";
import { TooltipProvider } from "../../../shared/ui/tooltip";
import { useSelectionStore } from "../../../stores/selectionStore";
import { useGestureRightsStore } from "../../store/gestureRightsStore";
import { NOTHING_REFUSED } from "../../model/runGestures";
import { useRunsPane } from "../../store/runsList/useRunsPane";
import RunTreeList from "./RunTreeList";

function runEntry(id: string, projectId: string, status = "running"): RunListEntry {
  return {
    run_id: id,
    name: `Run ${id}`,
    pipeline_name: "demo",
    status,
    effective_repo: "/repos/shop",
    project_id: projectId,
    stalled: false,
    started_at: new Date().toISOString(),
    activity: [],
  } as RunListEntry;
}

const PROJECTS = [
  { id: "p1", name: "Checkout", members: ["/repos/shop"] },
  { id: "p2", name: "Mobile", members: ["/repos/shop"] },
] as unknown as Project[];
const RUNS = [
  runEntry("c1", "p1"),
  runEntry("m1", "p2"),
  runEntry("m2", "p2", "awaiting_user"),
  runEntry("m3", "p2"),
];

function List({ selectedRunId = null }: { selectedRunId?: string | null }) {
  const pane = useRunsPane({ runs: RUNS, selectedRunId, onSelectRun: vi.fn(), projects: PROJECTS });
  return (
    <RunTreeList
      pane={pane}
      runs={RUNS}
      triggers={[]}
      selectedRunId={selectedRunId}
      onSelectRun={vi.fn()}
      onShowTriggers={vi.fn()}
    />
  );
}

function tree(selectedRunId: string | null = null) {
  return (
    <TooltipProvider>
      <ExtensionContextProvider selectedRun={null} runs={RUNS} projects={PROJECTS}>
        <List selectedRunId={selectedRunId} />
      </ExtensionContextProvider>
    </TooltipProvider>
  );
}

function mount() {
  return render(tree());
}

function group(label: string): HTMLElement {
  return screen
    .getAllByTestId("run-repo-group")
    .find((g) => within(g).getByTestId("run-repo-label").textContent === label)!;
}

beforeEach(() => {
  localStorage.clear();
  // jsdom does not lay out: the reveal of a selected row scrolls nowhere.
  Element.prototype.scrollIntoView = vi.fn();
  resetExtensions();
  useGestureRightsStore.setState({ rights: NOTHING_REFUSED, viewed: null, listed: new Set() });
  useSelectionStore.getState().clear("runs");
  vi.stubGlobal(
    "fetch",
    vi.fn(async () => new Response("[]", { status: 200, headers: { "Content-Type": "application/json" } })),
  );
});

describe("the fold of a Projet group", () => {
  it("hides the group's Runs, keeps its count and its awaiting ones, and survives a remount", () => {
    const first = mount();
    const mobile = group("Mobile");
    expect(within(mobile).getByTestId("run-group-count").textContent).toBe("3");
    expect(within(mobile).queryAllByTestId("run-display-label")).toHaveLength(3);
    expect(within(mobile).queryByTestId("run-group-awaiting")).toBeNull();

    fireEvent.click(within(mobile).getByTestId("run-group-chevron"));
    expect(group("Mobile").dataset.collapsed).toBe("true");
    expect(within(group("Mobile")).queryAllByTestId("run-display-label")).toHaveLength(0);
    expect(within(group("Mobile")).getByTestId("run-group-count").textContent).toBe("3 · 1 awaiting");
    expect(within(group("Mobile")).getByTestId("run-group-awaiting")).toBeTruthy();
    // The other group stays open.
    expect(within(group("Checkout")).queryAllByTestId("run-display-label")).toHaveLength(1);

    // A reload, or a new tab: the list mounts afresh, the fold is kept.
    first.unmount();
    mount();
    expect(group("Mobile").dataset.collapsed).toBe("true");
    expect(within(group("Mobile")).queryAllByTestId("run-display-label")).toHaveLength(0);

    fireEvent.click(within(group("Mobile")).getByTestId("run-group-chevron"));
    expect(within(group("Mobile")).queryAllByTestId("run-display-label")).toHaveLength(3);
    expect(JSON.parse(localStorage.getItem("pdo.ui.collapsedRunGroups")!)).toEqual([]);
  });

  it("unfolds the group of a Run selected from elsewhere, once", () => {
    localStorage.setItem("pdo.ui.collapsedRunGroups", JSON.stringify(["project:p2"]));
    const view = mount();
    expect(group("Mobile").dataset.collapsed).toBe("true");

    view.rerender(tree("m2"));
    expect(group("Mobile").dataset.collapsed).toBe("false");
    expect(within(group("Mobile")).queryAllByTestId("run-display-label")).toHaveLength(3);

    // Folded again by hand with the same selection: the fold sticks.
    fireEvent.click(within(group("Mobile")).getByTestId("run-group-chevron"));
    view.rerender(tree("m2"));
    expect(group("Mobile").dataset.collapsed).toBe("true");
  });

  it("keeps a fold made in another tab when folding here", () => {
    mount();
    localStorage.setItem("pdo.ui.collapsedRunGroups", JSON.stringify(["project:p1"]));
    fireEvent.click(within(group("Mobile")).getByTestId("run-group-chevron"));
    expect(JSON.parse(localStorage.getItem("pdo.ui.collapsedRunGroups")!).sort()).toEqual([
      "project:p1",
      "project:p2",
    ]);
  });
});
