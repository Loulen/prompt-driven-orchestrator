import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, within } from "@testing-library/react";
import SourceBranchField from "./SourceBranchField";
import type { BranchFetchError, BranchRef } from "../types";

/**
 * #802/ADR-0070. What a person sees and does with the source-branch control:
 * the quick pick, the écart chips, and the sync button that is the ONE place the
 * selected branch's écart is stated. Nothing here reads internal state.
 */

const NOW = Date.parse("2026-09-18T12:00:00Z");
const ago = (ms: number) => new Date(NOW - ms).toISOString();
const MIN = 60_000;
const HOUR = 3_600_000;
const DAY = 86_400_000;

const local = (name: string, over: Partial<BranchRef> = {}): BranchRef => ({
  name,
  kind: "local",
  last_commit_at: ago(3 * DAY),
  last_commit_subject: `work on ${name}`,
  ...over,
});
const remote = (name: string, over: Partial<BranchRef> = {}): BranchRef => ({
  name,
  kind: "remote",
  last_commit_at: ago(2 * HOUR),
  last_commit_subject: `work on ${name}`,
  ...over,
});
const tracking = (name: string, ahead: number, behind: number, over: Partial<BranchRef> = {}) =>
  local(name, { upstream: `origin/${name}`, ahead, behind, ...over });

const BRANCHES = [
  tracking("main", 0, 3, { last_commit_at: ago(3 * DAY), last_commit_subject: "chore(release): 1.86.2" }),
  tracking("develop", 0, 0, { last_commit_at: ago(5 * HOUR), last_commit_subject: "Merge pull request #799" }),
  local("local-only", { last_commit_at: ago(41 * MIN), last_commit_subject: "wip" }),
  remote("origin/feature-remote-only"),
];

function setup(over: Partial<React.ComponentProps<typeof SourceBranchField>> = {}) {
  const onChange = vi.fn();
  const onFetch = vi.fn();
  const utils = render(
    <SourceBranchField
      value="main"
      onChange={onChange}
      branches={BRANCHES}
      loading={false}
      fetching={false}
      lastFetchAt={ago(12_000)}
      fetchError={null}
      onFetch={onFetch}
      testIdPrefix="branch"
      ariaLabel="Source branch"
      {...over}
    />,
  );
  return { ...utils, onChange, onFetch };
}

const openPicker = () => fireEvent.click(screen.getByTestId("branch-trigger"));
const openSync = () => fireEvent.click(screen.getByTestId("branch-sync"));
const rows = () =>
  screen.getAllByTestId("branch-option").map((o) => o.getAttribute("data-branch"));

beforeEach(() => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
  vi.setSystemTime(NOW);
});

afterEach(() => {
  vi.useRealTimers();
});

describe("the field itself", () => {
  it("shows the chosen branch and the age of its tip", () => {
    setup();
    const trigger = screen.getByTestId("branch-trigger");
    expect(trigger).toHaveTextContent("main");
    expect(trigger).toHaveTextContent("3 d");
  });

  /**
   * The design decision of the #800 grilling: the écart lives in the sync button
   * and NOWHERE else. Two renderings of one number are free to disagree the moment
   * a refetch lands between renders.
   */
  it("never shows the écart in the field", () => {
    setup();
    expect(screen.getByTestId("branch-trigger")).not.toHaveTextContent("3↓");
    expect(screen.getByTestId("branch-sync-gap")).toHaveTextContent("3↓");
  });

  it("renders the held value verbatim even when the list does not carry it", () => {
    // The `origin/main` the sync shortcut switches to: deduped out of the list
    // (#571) yet legitimately launchable. A `<select>` would have silently shown
    // its first option instead — the #454 shows-one-sends-another trap.
    setup({ value: "origin/main" });
    expect(screen.getByTestId("branch-trigger")).toHaveTextContent("origin/main");
  });

  /**
   * After the shortcut switches to `origin/main` — a ref the list never carries —
   * the button must read it as a remote ref, not as "no upstream". The kind comes
   * from `main` declaring it as its upstream, never from the `/` in the name (a
   * LOCAL branch may be called `origin/x`, #571).
   */
  it("recognises an unlisted upstream as a remote ref", () => {
    setup({ value: "origin/main" });
    expect(screen.getByTestId("branch-sync")).toHaveAttribute("data-sync-state", "remote");
  });

  it("says so while the list is still loading", () => {
    setup({ value: "", branches: [], loading: true });
    expect(screen.getByTestId("branch-trigger")).toHaveTextContent("Loading branches…");
    expect(screen.getByTestId("branch-trigger")).toBeDisabled();
  });
});

describe("the quick pick", () => {
  it("lists locals before remotes, main first, then by recency", () => {
    setup();
    openPicker();
    expect(rows()).toEqual([
      "main",
      "local-only",
      "develop",
      "origin/feature-remote-only",
    ]);
  });

  it("shows the subject and the age of each branch's tip commit", () => {
    setup();
    openPicker();
    const row = screen.getAllByTestId("branch-option")[0];
    expect(row).toHaveTextContent("chore(release): 1.86.2");
    expect(row).toHaveTextContent("3 d");
  });

  it("chips the écart of tracking locals, and nothing else", () => {
    setup();
    openPicker();
    const chipOf = (name: string) =>
      within(
        screen.getAllByTestId("branch-option").find((o) => o.getAttribute("data-branch") === name)!,
      ).queryByTestId("branch-gap-chip");
    expect(chipOf("main")).toHaveTextContent("3↓");
    // Level, no-upstream and remote rows carry no numeric chip.
    expect(chipOf("develop")).toBeNull();
    expect(chipOf("local-only")).toBeNull();
    expect(chipOf("origin/feature-remote-only")).toBeNull();
  });

  it("counts the two groups and dates the list in its footer", () => {
    setup();
    openPicker();
    expect(screen.getByTestId("branch-footer")).toHaveTextContent(
      "3 local · 1 remote · fetched 12 s ago",
    );
  });

  it("says the date is worthless when the fetch failed", () => {
    setup({ fetchError: { kind: "network", message: "ssh: Could not resolve hostname" } });
    openPicker();
    expect(screen.getByTestId("branch-footer")).toHaveTextContent("fetched — failed");
  });

  it("filters on the name, case-insensitively", () => {
    setup();
    openPicker();
    fireEvent.change(screen.getByTestId("branch-search"), { target: { value: "ONLY" } });
    expect(rows()).toEqual(["local-only", "origin/feature-remote-only"]);
  });

  it("says so when nothing matches", () => {
    setup();
    openPicker();
    fireEvent.change(screen.getByTestId("branch-search"), { target: { value: "zzz" } });
    expect(screen.getByTestId("branch-empty")).toBeInTheDocument();
  });

  it("chooses a branch on click", () => {
    const { onChange } = setup();
    openPicker();
    fireEvent.mouseDown(
      screen.getAllByTestId("branch-option").find((o) => o.getAttribute("data-branch") === "develop")!,
    );
    expect(onChange).toHaveBeenCalledWith("develop");
    expect(screen.queryByTestId("branch-popover")).not.toBeInTheDocument();
  });

  it("walks the rows with ↑↓ and picks with ⏎, across the group boundary", () => {
    const { onChange } = setup();
    openPicker();
    const search = screen.getByTestId("branch-search");
    // Opens ON the selection (`main`, index 0); three ↓ reach the remote row.
    fireEvent.keyDown(search, { key: "ArrowDown" });
    fireEvent.keyDown(search, { key: "ArrowDown" });
    fireEvent.keyDown(search, { key: "ArrowDown" });
    fireEvent.keyDown(search, { key: "Enter" });
    expect(onChange).toHaveBeenCalledWith("origin/feature-remote-only");
  });

  it("opens on the current selection, not at the top of the list", () => {
    const { onChange } = setup({ value: "develop" });
    openPicker();
    fireEvent.keyDown(screen.getByTestId("branch-search"), { key: "Enter" });
    expect(onChange).toHaveBeenCalledWith("develop");
  });

  it("closes on Escape without changing anything", () => {
    const { onChange } = setup();
    openPicker();
    fireEvent.keyDown(screen.getByTestId("branch-search"), { key: "Escape" });
    expect(screen.queryByTestId("branch-popover")).not.toBeInTheDocument();
    expect(onChange).not.toHaveBeenCalled();
  });

  /**
   * Escape must reach the popover, not the dialog underneath it: dismissing a
   * half-filled New Run form because someone wanted to close a dropdown would be a
   * far worse surprise than one extra keystroke.
   */
  it("closes the sync popover on Escape too, and keeps the event to itself", () => {
    const onOuterEscape = vi.fn();
    document.addEventListener("keydown", onOuterEscape);
    try {
      setup();
      openSync();
      fireEvent.keyDown(screen.getByTestId("branch-sync"), { key: "Escape" });
      expect(screen.queryByTestId("branch-sync-popover")).not.toBeInTheDocument();
      expect(onOuterEscape).not.toHaveBeenCalled();
    } finally {
      document.removeEventListener("keydown", onOuterEscape);
    }
  });

  it("closes both popovers on a click outside", () => {
    setup();
    openSync();
    fireEvent.mouseDown(document.body);
    expect(screen.queryByTestId("branch-sync-popover")).not.toBeInTheDocument();
  });

  /**
   * The cursor must stay inside a list the filter just shortened, or ⏎ selects
   * nothing — or, worse, whatever slid into the old index.
   */
  it("keeps the cursor inside a list the filter has shortened", () => {
    const { onChange } = setup();
    openPicker();
    const search = screen.getByTestId("branch-search");
    fireEvent.keyDown(search, { key: "ArrowDown" });
    fireEvent.keyDown(search, { key: "ArrowDown" });
    fireEvent.keyDown(search, { key: "ArrowDown" });
    fireEvent.change(search, { target: { value: "develop" } });
    fireEvent.keyDown(search, { key: "Enter" });
    expect(onChange).toHaveBeenCalledWith("develop");
  });

  it("does not open the sync popover at the same time", () => {
    setup();
    openSync();
    expect(screen.getByTestId("branch-sync-popover")).toBeInTheDocument();
    openPicker();
    expect(screen.queryByTestId("branch-sync-popover")).not.toBeInTheDocument();
  });
});

describe("the sync button", () => {
  const stateOf = () => screen.getByTestId("branch-sync").getAttribute("data-sync-state");

  it("shows the selected branch's écart, and only then a label", () => {
    setup({ value: "main" });
    expect(stateOf()).toBe("gap");
    expect(screen.getByTestId("branch-sync-gap")).toHaveTextContent("3↓");
  });

  it("is icon-only when there is no écart to show", () => {
    setup({ value: "develop" });
    expect(stateOf()).toBe("level");
    expect(screen.queryByTestId("branch-sync-gap")).not.toBeInTheDocument();
  });

  it("names each state it can be in", () => {
    setup({ value: "origin/feature-remote-only" });
    expect(stateOf()).toBe("remote");
    setup({ value: "local-only" });
    expect(screen.getAllByTestId("branch-sync").at(-1)).toHaveAttribute(
      "data-sync-state",
      "no-upstream",
    );
  });

  it("spins while a fetch is in flight, whatever the refs say", () => {
    setup({ value: "develop", fetching: true });
    expect(stateOf()).toBe("fetching");
  });

  it("does not refetch on its own — a click only opens the state", () => {
    const { onFetch } = setup();
    openSync();
    expect(screen.getByTestId("branch-sync-popover")).toBeInTheDocument();
    expect(onFetch).not.toHaveBeenCalled();
  });
});

describe("the sync popover", () => {
  it("states the gap in words, dated", () => {
    setup({ value: "main" });
    openSync();
    const popover = screen.getByTestId("branch-sync-popover");
    expect(popover).toHaveTextContent("main is 3 commits behind origin/main");
    expect(popover).toHaveTextContent("Fetched 12 s ago.");
    expect(popover).toHaveTextContent("PDO never pulls into your checkout");
  });

  it("switches the selection to the upstream ref on the shortcut", () => {
    const { onChange } = setup({ value: "main" });
    openSync();
    fireEvent.click(screen.getByTestId("branch-sync-switch"));
    expect(onChange).toHaveBeenCalledWith("origin/main");
    expect(screen.queryByTestId("branch-sync-popover")).not.toBeInTheDocument();
  });

  it("refetches on demand", () => {
    const { onFetch } = setup();
    openSync();
    fireEvent.click(screen.getByTestId("branch-sync-fetch"));
    expect(onFetch).toHaveBeenCalledTimes(1);
  });

  it("confirms an up-to-date branch without offering to switch", () => {
    setup({ value: "develop" });
    openSync();
    expect(screen.getByTestId("branch-sync-popover")).toHaveTextContent(
      "Up to date with origin/develop",
    );
    expect(screen.queryByTestId("branch-sync-switch")).not.toBeInTheDocument();
  });

  it("names a single commit in the singular", () => {
    setup({ value: "one", branches: [tracking("one", 0, 1)] });
    openSync();
    expect(screen.getByTestId("branch-sync-popover")).toHaveTextContent(
      "one is 1 commit behind origin/one",
    );
  });

  it("names both axes of a divergence", () => {
    setup({ value: "two", branches: [tracking("two", 1, 4)] });
    openSync();
    expect(screen.getByTestId("branch-sync-popover")).toHaveTextContent(
      "two has diverged from origin/two — 1 ahead, 4 behind",
    );
  });

  it("names a branch that is only ahead", () => {
    setup({ value: "three", branches: [tracking("three", 2, 0)] });
    openSync();
    expect(screen.getByTestId("branch-sync-popover")).toHaveTextContent(
      "three is 2 commits ahead of origin/three",
    );
  });

  /**
   * ADR-0070 §1: a fetch that could not happen degrades the écart to "unknown,
   * as of the last successful fetch" and never blocks anything. The date and
   * git's own words are what let the user judge the numbers.
   */
  it("reports a failed fetch with its date and git's own words", () => {
    setup({
      value: "main",
      lastFetchAt: ago(2 * DAY),
      fetchError: {
        kind: "network",
        message: "ssh: Could not resolve hostname github.com: Temporary failure in name resolution",
      } satisfies BranchFetchError,
    });
    openSync();
    const popover = screen.getByTestId("branch-sync-popover");
    expect(popover).toHaveTextContent("Gap unknown — fetch failed");
    expect(popover).toHaveTextContent("Last successful fetch 2 d ago.");
    expect(popover).toHaveTextContent("Launching is still possible");
    expect(screen.getByTestId("branch-sync-stderr")).toHaveTextContent(
      "Temporary failure in name resolution",
    );
    // The escape hatch stays available precisely when the numbers are unknown.
    expect(screen.getByTestId("branch-sync-switch")).toBeInTheDocument();
  });

  it("admits when there has never been a successful fetch", () => {
    setup({
      value: "main",
      lastFetchAt: null,
      fetchError: { kind: "no_remote", message: "this repository has no remote" },
    });
    openSync();
    expect(screen.getByTestId("branch-sync-popover")).toHaveTextContent(
      "No successful fetch on record.",
    );
  });

  it("points at the ticket that will add the fast-forward", () => {
    setup();
    openSync();
    expect(screen.getByTestId("branch-sync-popover")).toHaveTextContent("fast-forward: #803");
  });
});

/**
 * #804 / ADR-0070 §1. A Trigger is a Run template that fires again and again, so
 * the one thing its source branch can be *wrong* about is being local: the fire
 * fetches before the cut, and a fetch cannot move a local branch. Everything here
 * is invisible in `run` mode, where the form has just fetched and the launch is
 * one-shot.
 */
describe("the Trigger's source branch (#804)", () => {
  const warning = () => screen.queryByTestId("branch-local-warning");

  it("warns on a local branch and points at the tracking ref to prefer", () => {
    setup({ purpose: "trigger", value: "main" });
    expect(warning()).toHaveAccessibleName(
      "Local branch — each fire cuts from its local state, which the pre-fire fetch does not move. Prefer origin/main.",
    );
    // Hover carries the same words: nothing here is clickable, the sentence IS
    // the affordance.
    expect(warning()).toHaveAttribute(
      "title",
      expect.stringContaining("the pre-fire fetch does not move"),
    );
  });

  it("drops the recommendation when the branch tracks nothing to prefer", () => {
    setup({ purpose: "trigger", value: "local-only" });
    expect(warning()).toHaveAccessibleName(
      "Local branch — each fire cuts from its local state, which the pre-fire fetch does not move.",
    );
  });

  it("says nothing about a tracking ref — that is the right choice", () => {
    setup({ purpose: "trigger", value: "origin/feature-remote-only" });
    expect(warning()).not.toBeInTheDocument();
  });

  it("never warns on a one-shot Run, local branch or not", () => {
    setup({ value: "main" });
    expect(warning()).not.toBeInTheDocument();
  });

  it("reserves the icon's width whatever is selected, so nothing jumps", () => {
    // Switching a Trigger's source from `main` to `origin/main` is one click in
    // the popover; the field must not shift under the cursor while it happens.
    const local = setup({ purpose: "trigger", value: "main" });
    expect(within(local.container).getByTestId("branch-local-warning-slot")).toBeInTheDocument();
    expect(within(local.container).getByTestId("branch-local-warning")).toBeInTheDocument();

    const remote = setup({ purpose: "trigger", value: "origin/feature-remote-only" });
    expect(within(remote.container).getByTestId("branch-local-warning-slot")).toBeInTheDocument();
    expect(
      within(remote.container).queryByTestId("branch-local-warning"),
    ).not.toBeInTheDocument();
  });

  it("costs the Run form nothing — no slot at all outside Trigger mode", () => {
    setup({ value: "main" });
    expect(screen.queryByTestId("branch-local-warning-slot")).not.toBeInTheDocument();
  });

  it("tells the sync popover of a tracking ref what a fire will do about it", () => {
    setup({ purpose: "trigger", value: "origin/feature-remote-only" });
    openSync();
    const popover = screen.getByTestId("branch-sync-popover");
    expect(popover).toHaveTextContent("Fetched before the cut on each fire.");
    // Story 21: a failed fetch must not read as a lost fire.
    expect(popover).toHaveTextContent("A failed fetch still fires");
    expect(popover).toHaveTextContent("records the reason");
  });

  it("keeps that sentence out of the Run form", () => {
    setup({ value: "origin/feature-remote-only" });
    openSync();
    expect(screen.queryByTestId("branch-sync-trigger-note")).not.toBeInTheDocument();
  });
});
