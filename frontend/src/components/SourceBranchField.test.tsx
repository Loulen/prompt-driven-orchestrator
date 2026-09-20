import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor, within } from "@testing-library/react";
import SourceBranchField from "./SourceBranchField";
import type {
  BranchFetchError,
  BranchRef,
  FastForwardOutcome,
  FastForwardRefusal,
  FastForwardResult,
} from "../types";

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

  it("states what a fast-forward will not do", () => {
    setup();
    openSync();
    // ADR-0070 §2 in the footer of every state: the promise is what makes the
    // button safe to press without reading a confirmation dialog.
    expect(screen.getByTestId("branch-sync-popover")).toHaveTextContent(
      "never merges, rebases or pushes",
    );
  });
});

/**
 * #803/ADR-0070 §2 — the fast-forward. Optimistic at the click, named at the
 * refusal: the button shows on what the list already knows, and the two refusals
 * that need a checkout inspected come back from the daemon.
 */
describe("the fast-forward", () => {
  const ffSetup = (
    outcome: FastForwardOutcome | null,
    over: Partial<React.ComponentProps<typeof SourceBranchField>> = {},
  ) => {
    const onFastForward = vi.fn().mockResolvedValue(outcome);
    const utils = setup({ onFastForward, ...over });
    return { ...utils, onFastForward };
  };

  const done = (over: Partial<FastForwardResult> = {}): FastForwardOutcome => ({
    kind: "done",
    result: {
      branch: "main",
      upstream: "origin/main",
      from: "a1b2c3d",
      to: "e4f5a6b",
      commits: 3,
      subject: "fix(daemon): prune on teardown",
      moved_checkout: "/home/me/prompt-driven-orchestrator",
      ...over,
    },
    list: { branches: [], last_fetch_at: null, fetch_error: null },
  });

  const refused = (refusal: FastForwardRefusal): FastForwardOutcome => ({
    kind: "refused",
    refusal,
    list: { branches: [], last_fetch_at: null, fetch_error: null },
  });

  it("offers the fast-forward on a branch that is strictly behind, and leads with it", () => {
    ffSetup(done());
    openSync();
    const popover = screen.getByTestId("branch-sync-popover");
    const buttons = within(popover).getAllByRole("button");
    // The order IS the recommendation: the fast-forward repairs what the person
    // already chose, the #802 shortcut steps down to the fallback it now is.
    expect(buttons[0]).toHaveAttribute("data-testid", "branch-sync-ff");
    expect(buttons[0]).toHaveTextContent("Fast-forward main");
    expect(buttons[1]).toHaveAttribute("data-testid", "branch-sync-switch");
  });

  /**
   * Three of the five refusals are knowable from the list, so the button is never
   * offered for them — the sentence explains instead of a click discovering it.
   */
  it.each([
    ["diverged", tracking("main", 1, 3)],
    ["ahead only", tracking("main", 2, 0)],
    ["up to date", tracking("main", 0, 0)],
    ["no upstream", local("main")],
  ])("offers no fast-forward when the list already says %s", (_case, branch) => {
    ffSetup(done(), { branches: [branch] });
    openSync();
    expect(screen.queryByTestId("branch-sync-ff")).not.toBeInTheDocument();
  });

  it("offers no fast-forward when the écart is unknown", () => {
    // A failed fetch dates every number from the last success: advancing onto a
    // ref nobody could refresh is the staleness this feature removes.
    ffSetup(done(), {
      fetchError: { kind: "network", message: "could not resolve host" },
    });
    openSync();
    expect(screen.queryByTestId("branch-sync-ff")).not.toBeInTheDocument();
  });

  it("never fast-forwards on its own — only the button does", () => {
    const { onFastForward } = ffSetup(done());
    openSync();
    fireEvent.click(screen.getByTestId("branch-sync-fetch"));
    expect(onFastForward).not.toHaveBeenCalled();
  });

  it("reports what moved, and keeps the popover open to be read", async () => {
    ffSetup(done());
    openSync();
    fireEvent.click(screen.getByTestId("branch-sync-ff"));
    const popover = await screen.findByTestId("branch-sync-popover");
    expect(popover).toHaveTextContent("main fast-forwarded to origin/main");
    expect(popover).toHaveTextContent("3 commits");
    expect(popover).toHaveTextContent("now sits on the new tip");
    expect(screen.getByTestId("branch-sync-ff-moved")).toHaveTextContent(
      "a1b2c3d → e4f5a6b · fix(daemon): prune on teardown",
    );
  });

  it("says no file changed when the branch was checked out nowhere", async () => {
    ffSetup(done({ moved_checkout: null }));
    openSync();
    fireEvent.click(screen.getByTestId("branch-sync-ff"));
    expect(await screen.findByTestId("branch-sync-popover")).toHaveTextContent(
      "No file changed",
    );
  });

  it("promises the working tree will not move when HEAD is another branch", () => {
    ffSetup(done(), { branches: [tracking("main", 0, 3, { head: false })] });
    openSync();
    expect(screen.getByTestId("branch-sync-popover")).toHaveTextContent(
      "none of your files change",
    );
  });

  it("names a dirty-tree refusal, lists the files, and offers a retry", async () => {
    ffSetup(
      refused({
        reason: "dirty_tree",
        message: "two tracked files are modified",
        checkout: "/home/me/prompt-driven-orchestrator",
        dirty_files: ["M crates/pdo-daemon/src/git.rs", "M frontend/src/lib/branchSelect.ts"],
        dirty_count: 2,
      }),
    );
    openSync();
    fireEvent.click(screen.getByTestId("branch-sync-ff"));

    const popover = await screen.findByTestId("branch-sync-popover");
    expect(popover).toHaveTextContent("working tree not clean");
    // The daemon's own reason code: what a person searches for, and what a bug
    // report should carry.
    expect(screen.getByTestId("branch-sync-ff-reason")).toHaveTextContent("dirty_tree");
    expect(screen.getByTestId("branch-sync-ff-dirty")).toHaveTextContent("branchSelect.ts");
    // The shortcut leads: it is the exit that works whatever the checkout is doing.
    expect(within(popover).getAllByRole("button")[0]).toHaveAttribute(
      "data-testid",
      "branch-sync-switch",
    );
    expect(screen.getByTestId("branch-sync-ff-retry")).toBeInTheDocument();
  });

  it("names a branch held by another worktree, and offers no retry", async () => {
    ffSetup(
      refused({
        reason: "checked_out_elsewhere",
        message: "checked out elsewhere",
        checkout: "/home/me/pdo/.pdo/runs/2026-x/worktree",
      }),
    );
    openSync();
    fireEvent.click(screen.getByTestId("branch-sync-ff"));

    const popover = await screen.findByTestId("branch-sync-popover");
    // The popover is already open before the refusal lands: wait for the copy,
    // not for the element (flaked under full-suite load otherwise).
    await waitFor(() => expect(popover).toHaveTextContent("checked out in another worktree"));
    expect(popover).toHaveTextContent("…/2026-x/worktree");
    // Retrying changes nothing until someone switches that worktree: offering it
    // would be an invitation to a loop.
    expect(screen.queryByTestId("branch-sync-ff-retry")).not.toBeInTheDocument();
    expect(screen.getByTestId("branch-sync-switch")).toBeInTheDocument();
  });

  /**
   * The refusal the popover's own precondition said was impossible. The list is only
   * as fresh as the last read, so a commit made in a terminal while the popover sits
   * open turns "1 behind, 0 ahead" into a divergence and the button is still on offer.
   *
   * The first version of the card tested for `dirty_tree` and gave EVERYTHING else
   * the "checked out in another worktree" copy, so this 409 told the reader their
   * branch was held by a worktree that did not exist — sending them to hunt for it
   * (found by the #803 Feature Path).
   */
  it("names a divergence as a divergence, not as a worktree", async () => {
    ffSetup(
      refused({
        reason: "diverged",
        message: "'main' has 1 commit(s) 'origin/main' does not: a fast-forward would drop them",
      }),
    );
    openSync();
    fireEvent.click(screen.getByTestId("branch-sync-ff"));

    const popover = await screen.findByTestId("branch-sync-popover");
    expect(popover).toHaveTextContent("the branch has diverged");
    expect(popover).not.toHaveTextContent("another worktree");
    expect(popover).not.toHaveTextContent("another checkout");
    expect(screen.getByTestId("branch-sync-ff-reason")).toHaveTextContent("diverged");
    // The counts live in the daemon's sentence and nowhere else on this card.
    expect(screen.getByTestId("branch-sync-ff-message")).toHaveTextContent(
      "has 1 commit(s) 'origin/main' does not",
    );
    // Reconciling is the exit, and it is the person's to make: PDO never merges.
    expect(popover).toHaveTextContent("Reconcile in your terminal");
    expect(screen.queryByTestId("branch-sync-ff-retry")).not.toBeInTheDocument();
    expect(screen.getByTestId("branch-sync-switch")).toBeInTheDocument();
  });

  it("invents no cause for a reason it does not know", async () => {
    // A daemon newer than this build. Quoting it is honest; picking a neighbour's
    // sentence is how the diverged card got its wrong worktree.
    ffSetup(
      refused({
        reason: "locked_index" as FastForwardRefusal["reason"],
        message: "index.lock exists: another git process is running",
      }),
    );
    openSync();
    fireEvent.click(screen.getByTestId("branch-sync-ff"));

    const popover = await screen.findByTestId("branch-sync-popover");
    expect(popover).toHaveTextContent("the daemon refused");
    expect(popover).not.toHaveTextContent("worktree");
    expect(screen.getByTestId("branch-sync-ff-message")).toHaveTextContent("index.lock exists");
  });

  /**
   * The two benign races. Someone pulled (or unset the upstream) between the render
   * and the click: the refreshed list that came back already says so, and painting
   * an error for a beat of latency would be a lie about the repository.
   */
  it.each([["up_to_date"], ["no_upstream"]] as const)(
    "shows no error card for a %s race",
    async (reason) => {
      ffSetup(refused({ reason, message: "already up to date" }));
      openSync();
      fireEvent.click(screen.getByTestId("branch-sync-ff"));

      const popover = await screen.findByTestId("branch-sync-popover");
      expect(popover).toHaveAttribute("data-ff-state", "idle");
      expect(popover).not.toHaveTextContent("Can't fast-forward");
      expect(screen.queryByTestId("branch-sync-ff-reason")).not.toBeInTheDocument();
    },
  );

  it("says nothing moved when the call itself fails", async () => {
    ffSetup({ kind: "error", message: "daemon unreachable" });
    openSync();
    fireEvent.click(screen.getByTestId("branch-sync-ff"));

    const popover = await screen.findByTestId("branch-sync-popover");
    expect(popover).toHaveTextContent("Nothing moved");
    expect(screen.getByTestId("branch-sync-ff-error")).toHaveTextContent("daemon unreachable");
    // A failed fast-forward never blocks a launch (same contract as a failed fetch).
    expect(screen.getByTestId("branch-sync-switch")).toBeInTheDocument();
  });

  it("freezes the popover while the fast-forward is in flight", async () => {
    let release: (o: FastForwardOutcome) => void = () => {};
    const onFastForward = vi.fn(
      () => new Promise<FastForwardOutcome>((r) => (release = r)),
    );
    setup({ onFastForward });
    openSync();
    fireEvent.click(screen.getByTestId("branch-sync-ff"));

    const popover = await screen.findByTestId("branch-sync-popover");
    expect(popover).toHaveAttribute("data-ff-state", "running");
    expect(popover).toHaveTextContent("Fast-forwarding main…");
    // Deferred, not removed: the layout must not reshuffle under the cursor.
    expect(screen.getByTestId("branch-sync-ff")).toBeDisabled();
    expect(screen.getByTestId("branch-sync-switch")).toBeDisabled();
    // The trigger drops the écart it is in the middle of erasing.
    expect(screen.getByTestId("branch-sync")).toHaveAttribute(
      "data-sync-state",
      "fast-forwarding",
    );

    release(done());
    expect(await screen.findByTestId("branch-sync-ff-moved")).toBeInTheDocument();
  });

  it("forgets the result when the selection changes", async () => {
    const { rerender, onChange, onFetch } = ffSetup(done());
    openSync();
    fireEvent.click(screen.getByTestId("branch-sync-ff"));
    await screen.findByTestId("branch-sync-ff-moved");

    // A card about `main` must not survive onto `develop`: the refreshed list
    // already carries the outcome that outlives it (the ✓ on the button).
    rerender(
      <SourceBranchField
        value="develop"
        onChange={onChange}
        branches={BRANCHES}
        loading={false}
        fetching={false}
        lastFetchAt={ago(12_000)}
        fetchError={null}
        onFetch={onFetch}
        testIdPrefix="branch"
        ariaLabel="Source branch"
      />,
    );
    expect(screen.queryByTestId("branch-sync-ff-moved")).not.toBeInTheDocument();
  });

  /**
   * A result card describes a list the fetch is on its way to replace. Left standing
   * it outlives what it describes: the chip flips back to `1↓` while the body still
   * reads "fast-forwarded", and the fast-forward button — which lives in the
   * freshness card, not this one — is unreachable until the popover is closed and
   * reopened (found by the #803 Feature Path).
   */
  it("retires the result when you ask the question again", async () => {
    const { onFetch } = ffSetup(done());
    openSync();
    fireEvent.click(screen.getByTestId("branch-sync-ff"));
    await screen.findByTestId("branch-sync-ff-moved");

    fireEvent.click(screen.getByTestId("branch-sync-fetch"));
    expect(onFetch).toHaveBeenCalledTimes(1);
    expect(screen.queryByTestId("branch-sync-ff-moved")).not.toBeInTheDocument();
    const popover = screen.getByTestId("branch-sync-popover");
    expect(popover).toHaveAttribute("data-ff-state", "idle");
    // The freshness card is back, and with it the button the stale card hid.
    expect(screen.getByTestId("branch-sync-ff")).toBeInTheDocument();
  });

  /**
   * #803 — the popover's numbers now decide whether a button that MOVES A BRANCH is
   * offered, so they may not be a count cached when the modal opened. Refs move
   * outside PDO; re-reading them is local, and is not the fetch the footer promises.
   */
  it("re-reads the local refs when it opens", () => {
    const onReread = vi.fn();
    const { onFetch } = ffSetup(done(), { onReread });
    openSync();
    expect(onReread).toHaveBeenCalledTimes(1);
    expect(onFetch).not.toHaveBeenCalled();

    // Closing asks nothing, and the next opening asks again.
    openSync();
    expect(onReread).toHaveBeenCalledTimes(1);
    openSync();
    expect(onReread).toHaveBeenCalledTimes(2);
  });

  it("opens without a re-read when the call site has nothing to re-read", () => {
    ffSetup(done());
    openSync();
    expect(screen.getByTestId("branch-sync-popover")).toBeInTheDocument();
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
