/**
 * The Runs list block of Settings › Appearance: a switch per element of a row,
 * the extensions' contributions by name and label, and beside them a preview
 * made of the list's own rows, on the most recent Run the user sees or on a
 * sample. Pointing a switch rings its element in the preview.
 */
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ExtensionEntry } from "../../../shared/generated/ExtensionEntry";
import type { RunListEntry } from "../../../shared/generated/RunListEntry";
import type { UiContribution } from "../../../shared/generated/UiContribution";
import ExtensionContextProvider from "../../../shared/extensions/ExtensionContextProvider";
import { hostReactMajor } from "../../../shared/extensions/shared";
import { resetExtensions, setBundleLoader } from "../../../shared/extensions/store";
import { resetRunCosts } from "../../../run/store/runCosts";
import { resetAppearance, useAppearance } from "../../../hooks/useAppearance";
import RunsListBlock from "./RunsListBlock";
import { isOn, rowOf, switchOf } from "./testing";

const run = (run_id: string, name: string, started: string, over: Partial<RunListEntry> = {}) =>
  ({
    run_id,
    name,
    pipeline_name: `${run_id}-pipeline`,
    status: "completed",
    stalled: false,
    started_at: started,
    completed_at: "2026-01-01T13:00:00Z",
    effective_repo: "/r",
    ...over,
  }) as RunListEntry;

const RUNS = [
  run("older", "Older", "2026-01-01T09:00:00Z"),
  run("recent", "Implement store", "2026-01-01T12:00:00Z"),
];

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

function serve(extensions: ExtensionEntry[]) {
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string) => {
      const u = String(url);
      const body =
        u === "/ext"
          ? extensions
          : u.includes("/runs/costs")
            ? {
                runs: RUNS.map((r) => ({
                  run_id: r.run_id,
                  cost: { usd: 2.41, partial: false, unpriced_models: [], uncosted_harnesses: [], by_harness: [] },
                })),
              }
            : u.includes("presences")
              ? { runs: [] }
              : {};
      return new Response(JSON.stringify(body), { status: 200, headers: { "Content-Type": "application/json" } });
    }),
  );
}

/** The Actor's element deviations, as the Appearance store holds them. */
function Probe() {
  return <output data-testid="probe">{JSON.stringify(useAppearance().elements)}</output>;
}
const deviations = () => JSON.parse(screen.getByTestId("probe").textContent ?? "{}");

function mount(runs: RunListEntry[] = RUNS) {
  return render(
    <ExtensionContextProvider selectedRun={null} runs={runs} projects={[]}>
      <RunsListBlock runs={runs} triggers={[]} />
      <Probe />
    </ExtensionContextProvider>,
  );
}

const preview = () => screen.getByTestId("appearance-preview-runs-list");
const previewRow = (id: string) => preview().querySelector(`[data-run-row="${id}"]`) as HTMLElement;

beforeEach(() => {
  localStorage.clear();
  resetAppearance();
  resetExtensions();
  resetRunCosts();
  setBundleLoader(async () => ({
    Badge: () => <span data-testid="badge">T-1</span>,
    People: () => <span data-testid="people">AL</span>,
  }));
  serve([EXTENSION]);
});

afterEach(() => {
  vi.unstubAllGlobals();
  resetExtensions();
});

describe("the Runs list block", () => {
  it("says what it always shows, and groups its switches by line, at their defaults", () => {
    mount();
    const block = screen.getByTestId("appearance-block-runs-list");
    expect(block).toHaveTextContent("Runs list");
    expect(block).toHaveTextContent("Always shown: Status, name");
    expect(block).toHaveTextContent("All hidden: rows take one line");
    const on = (id: string) => isOn(switchOf(`runs.list.${id}`));
    expect(["trigger", "pipeline", "duration"].map(on)).toEqual([true, true, true]);
    expect(["child-pills", "node-pips", "parent", "cost"].map(on)).toEqual([false, false, false, false]);
  });

  it("lists each extension contribution by its extension's name and its label, shown by default", async () => {
    mount();
    expect(await screen.findByTestId("setting-row-runs.list.ext.test:badge")).toHaveTextContent("Test · Badge");
    expect(rowOf("runs.list.ext.test:people")).toHaveTextContent("Test · People");
    expect(isOn(switchOf("runs.list.ext.test:badge"))).toBe(true);
    expect(isOn(switchOf("runs.list.ext.test:people"))).toBe(true);
  });

  it("hides one contribution at a time: only its deviation is kept", async () => {
    mount();
    await within(previewRow("recent")).findByTestId("badge");
    fireEvent.click(await screen.findByRole("switch", { name: "Test · Badge" }));
    expect(deviations()).toEqual({ "runs.list.ext.test:badge": false });
    expect(within(previewRow("recent")).queryByTestId("badge")).toBeNull();
    expect(within(previewRow("recent")).getByTestId("people")).toBeInTheDocument();
  });

  it("previews the most recent Run the user sees, as the list draws it", async () => {
    mount();
    expect(preview()).toHaveTextContent("Your most recent run");
    expect(preview().querySelectorAll("[data-run-row]")).toHaveLength(1);
    const row = within(previewRow("recent"));
    expect(row.getByTestId("run-display-label")).toHaveTextContent("Implement store");
    expect(row.getByTestId("run-pipeline-name")).toHaveTextContent("recent-pipeline");
    expect(await row.findByTestId("badge")).toBeInTheDocument();
    // Inert: no hover bar, out of the tab order.
    expect(row.queryByTestId("run-row-actions")).toBeNull();
    expect(previewRow("recent")).toHaveAttribute("tabindex", "-1");
  });

  it("follows each switch at once, says « changed » with the default in its tooltip", async () => {
    mount();
    fireEvent.click(switchOf("runs.list.cost"));
    expect(await within(previewRow("recent")).findByTestId("run-cost")).toHaveTextContent("$2.41");
    const changed = screen.getByTestId("setting-row-runs.list.cost-changed");
    expect(changed).toHaveTextContent("changed");
    expect(changed).toHaveAttribute("title", "Default: Hidden");
    fireEvent.click(switchOf("runs.list.cost"));
    expect(screen.queryByTestId("setting-row-runs.list.cost-changed")).toBeNull();
    expect(deviations()).toEqual({});
  });

  it("puts the preview on one line once the whole second line is hidden", async () => {
    mount();
    await within(previewRow("recent")).findByTestId("badge");
    expect(previewRow("recent")).toHaveAttribute("data-lines", "2");
    for (const name of ["Pipeline", "Duration", "Test · Badge", "Test · People"]) {
      // The pointer and the focus stay on the switch just turned off, as a hand leaves them.
      const control = screen.getByRole("switch", { name });
      fireEvent.pointerEnter(control.closest(".set-ablock__point")!);
      fireEvent.focus(control);
      fireEvent.click(control);
    }
    expect(previewRow("recent")).toHaveAttribute("data-lines", "1");
    expect(previewRow("recent").querySelector(".lp-l2")).toBeNull();
  });

  it("rings the element a switch points to, and brings a hidden one back as a ghost", async () => {
    mount();
    fireEvent.pointerEnter(rowOf("runs.list.pipeline").parentElement!);
    const pipeline = within(previewRow("recent")).getByTestId("run-pipeline-name");
    expect(pipeline).toHaveAttribute("data-pointed");
    expect(pipeline).not.toHaveAttribute("data-ghost");

    fireEvent.pointerEnter(rowOf("runs.list.cost").parentElement!);
    const cost = await within(previewRow("recent")).findByTestId("run-cost");
    expect(cost).toHaveAttribute("data-pointed");
    expect(cost).toHaveAttribute("data-ghost");
    expect(pipeline).not.toHaveAttribute("data-pointed");

    fireEvent.pointerLeave(rowOf("runs.list.cost").parentElement!);
    expect(within(previewRow("recent")).queryByTestId("run-cost")).toBeNull();
  });

  it("points by the keyboard focus too", () => {
    mount();
    fireEvent.focus(switchOf("runs.list.duration"));
    expect(within(previewRow("recent")).getByTestId("run-duration")).toHaveAttribute("data-pointed");
    fireEvent.blur(switchOf("runs.list.duration"));
    expect(within(previewRow("recent")).getByTestId("run-duration")).not.toHaveAttribute("data-pointed");
  });

  it("shows a parent with its child, so the child pills and the parent link appear once", async () => {
    const runs = [
      ...RUNS,
      run("parent", "Review", "2026-01-01T10:00:00Z"),
      run("child", "Slice", "2026-01-01T10:05:00Z", { parent_run_id: "parent" }),
    ];
    mount(runs);
    expect(preview()).toHaveTextContent("Your most recent run, and a parent with its child");
    expect([...preview().querySelectorAll("[data-run-row]")].map((r) => r.getAttribute("data-run-row"))).toEqual([
      "recent",
      "parent",
      "child",
    ]);
    fireEvent.click(switchOf("runs.list.child-pills"));
    fireEvent.click(switchOf("runs.list.parent"));
    expect(within(previewRow("parent")).getByTestId("run-child-pills")).toBeInTheDocument();
    expect(within(previewRow("child")).getByTestId("run-parent-link")).toHaveTextContent("Review");
  });

  it("shows a sample Run without extension marks when the user sees no Run", async () => {
    mount([]);
    await screen.findByRole("switch", { name: "Test · Badge" });
    expect(preview()).toHaveTextContent("Sample run · you see no run yet");
    expect(preview()).toHaveTextContent("Sample run");
    await waitFor(() => expect(fetch).toHaveBeenCalledWith("/ext", expect.anything()));
    expect(within(preview()).queryByTestId("badge")).toBeNull();
    expect(within(preview()).queryByTestId("people")).toBeNull();
    expect(within(preview()).getByTestId("run-trigger-badge")).toBeInTheDocument();
    // The sample asks the daemon for no cost.
    fireEvent.click(switchOf("runs.list.cost"));
    expect(within(preview()).getByTestId("run-cost")).toHaveTextContent("$0.42");
    expect(vi.mocked(fetch).mock.calls.some(([url]) => String(url).includes("/runs/costs"))).toBe(false);
  });
});
