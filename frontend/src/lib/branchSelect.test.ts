import { describe, it, expect } from "vitest";
import type { BranchRef, FastForwardRefusal } from "../types";
import {
  branchGap,
  canFastForward,
  fastForwardLabel,
  fastForwardTouchesCheckout,
  filterBranches,
  gapLabel,
  groupBranches,
  highlightSegments,
  isBenignRefusal,
  isRetryableRefusal,
  pickDefaultBranch,
  shortAge,
  syncState,
  upstreamSwitchTarget,
} from "./branchSelect";

const local = (name: string, over: Partial<BranchRef> = {}): BranchRef => ({
  name,
  kind: "local",
  ...over,
});
const remote = (name: string, over: Partial<BranchRef> = {}): BranchRef => ({
  name,
  kind: "remote",
  ...over,
});

/** A local branch that tracks `origin/<name>` and is `ahead`/`behind` by these. */
const tracking = (name: string, ahead: number, behind: number, over: Partial<BranchRef> = {}) =>
  local(name, { upstream: `origin/${name}`, ahead, behind, ...over });

describe("pickDefaultBranch (#454/#571)", () => {
  it("prefers main, then master, then the first local", () => {
    expect(pickDefaultBranch([local("dev"), local("master"), local("main")])).toBe("main");
    expect(pickDefaultBranch([local("dev"), local("master")])).toBe("master");
    expect(pickDefaultBranch([local("topic"), local("other")])).toBe("topic");
  });

  it("never picks a remote while any local exists (#571)", () => {
    expect(
      pickDefaultBranch([local("master"), remote("origin/main"), remote("origin/master")]),
    ).toBe("master");
  });

  it("falls through to a remote only with no local at all (#571)", () => {
    expect(pickDefaultBranch([remote("origin/dev"), remote("origin/main")])).toBe("origin/main");
    expect(pickDefaultBranch([remote("origin/dev"), remote("origin/master")])).toBe(
      "origin/master",
    );
    expect(pickDefaultBranch([remote("upstream/topic")])).toBe("upstream/topic");
  });

  it("has nothing to offer for an empty list", () => {
    expect(pickDefaultBranch([])).toBeUndefined();
  });

  /**
   * ADR-0070 §3: showing the écart must not rewrite the choice. A `main` that is
   * three commits behind is still the default — the user is now told, and decides.
   */
  it("still defaults to a local that is behind its upstream (#802)", () => {
    expect(pickDefaultBranch([tracking("main", 0, 3), remote("origin/other")])).toBe("main");
  });
});

describe("groupBranches (#802)", () => {
  it("puts main and master at the head of each group", () => {
    const { locals } = groupBranches([
      local("zeta", { last_commit_at: "2026-09-18T10:00:00Z" }),
      local("master", { last_commit_at: "2020-01-01T00:00:00Z" }),
      local("main", { last_commit_at: "2019-01-01T00:00:00Z" }),
    ]);
    expect(locals.map((b) => b.name)).toEqual(["main", "master", "zeta"]);
  });

  it("orders the rest by recency of the tip commit", () => {
    const { locals } = groupBranches([
      local("old", { last_commit_at: "2026-09-01T00:00:00Z" }),
      local("newest", { last_commit_at: "2026-09-18T00:00:00Z" }),
      local("middle", { last_commit_at: "2026-09-10T00:00:00Z" }),
    ]);
    expect(locals.map((b) => b.name)).toEqual(["newest", "middle", "old"]);
  });

  /**
   * A dateless branch sorting anywhere-in-particular would make the list reshuffle
   * between renders; "unknown is not recent" gives it one stable place — last.
   */
  it("sorts a branch with no date last, and breaks ties by name", () => {
    const { locals } = groupBranches([
      local("no-date"),
      local("b", { last_commit_at: "2026-09-10T00:00:00Z" }),
      local("a", { last_commit_at: "2026-09-10T00:00:00Z" }),
    ]);
    expect(locals.map((b) => b.name)).toEqual(["a", "b", "no-date"]);
  });

  it("strips the remote prefix before testing for main (#571 locality)", () => {
    const { remotes } = groupBranches([
      remote("origin/zeta", { last_commit_at: "2026-09-18T00:00:00Z" }),
      remote("origin/main", { last_commit_at: "2019-01-01T00:00:00Z" }),
    ]);
    expect(remotes.map((b) => b.name)).toEqual(["origin/main", "origin/zeta"]);
  });

  it("separates locals from remotes without re-deriving locality by name", () => {
    // A LOCAL branch may legitimately be called `origin/x` (#571): `kind` decides.
    const { locals, remotes } = groupBranches([local("origin/ambig"), remote("origin/real")]);
    expect(locals.map((b) => b.name)).toEqual(["origin/ambig"]);
    expect(remotes.map((b) => b.name)).toEqual(["origin/real"]);
  });
});

describe("filterBranches (#802)", () => {
  const list = [local("main"), local("feature/800-sync"), remote("origin/FEATURE-x")];

  it("matches a case-insensitive substring of the name", () => {
    expect(filterBranches(list, "FEAT").map((b) => b.name)).toEqual([
      "feature/800-sync",
      "origin/FEATURE-x",
    ]);
  });

  /**
   * Not on the commit subject: typing `fix` would surface every branch whose last
   * commit says "fix", which is most of them, and the list would stop being a way
   * to find a branch you can name.
   */
  it("ignores the commit subject", () => {
    const withSubject = [local("main", { last_commit_subject: "fix the parser" })];
    expect(filterBranches(withSubject, "parser")).toEqual([]);
  });

  it("preserves order — a filter that re-ranks moves the row under the cursor", () => {
    expect(filterBranches(list, "").map((b) => b.name)).toEqual(list.map((b) => b.name));
  });
});

describe("highlightSegments (#802)", () => {
  it("splits the name around the matched run", () => {
    expect(highlightSegments("integration/800-sync", "800")).toEqual([
      { text: "integration/", match: false },
      { text: "800", match: true },
      { text: "-sync", match: false },
    ]);
  });

  it("returns the whole name when nothing matches, and drops empty pieces", () => {
    expect(highlightSegments("main", "")).toEqual([{ text: "main", match: false }]);
    expect(highlightSegments("main", "zzz")).toEqual([{ text: "main", match: false }]);
    expect(highlightSegments("main", "main")).toEqual([{ text: "main", match: true }]);
  });
});

describe("branchGap (#802)", () => {
  it("names each shape of the écart", () => {
    expect(branchGap(tracking("main", 0, 0))).toEqual({ kind: "level" });
    expect(branchGap(tracking("main", 2, 0))).toEqual({ kind: "ahead", ahead: 2 });
    expect(branchGap(tracking("main", 0, 3))).toEqual({ kind: "behind", behind: 3 });
    expect(branchGap(tracking("main", 1, 4))).toEqual({
      kind: "diverged",
      ahead: 1,
      behind: 4,
    });
  });

  /**
   * The distinction the whole chip rests on: a branch that tracks NOTHING shows no
   * chip, where `0↑ 0↓` shows a ✓. Collapsing the two would paint "up to date" on
   * every branch that has nothing to be up to date with.
   */
  it("has nothing to say about a branch with no upstream", () => {
    expect(branchGap(local("solo"))).toBeNull();
    expect(branchGap(remote("origin/main"))).toBeNull();
    expect(branchGap(undefined)).toBeNull();
  });

  it("calls the écart unknown when the counts are missing but the upstream is not", () => {
    expect(branchGap(local("main", { upstream: "origin/main" }))).toEqual({ kind: "unknown" });
  });

  it("labels only the numeric shapes", () => {
    expect(gapLabel(branchGap(tracking("main", 2, 3)))).toBe("2↑ 3↓");
    expect(gapLabel(branchGap(tracking("main", 0, 3)))).toBe("3↓");
    expect(gapLabel(branchGap(tracking("main", 2, 0)))).toBe("2↑");
    expect(gapLabel(branchGap(tracking("main", 0, 0)))).toBe("");
    expect(gapLabel(null)).toBe("");
  });
});

describe("shortAge (#802)", () => {
  const now = Date.parse("2026-09-18T12:00:00Z");
  const ago = (ms: number) => new Date(now - ms).toISOString();

  it("uses one scale of English short units", () => {
    expect(shortAge(ago(12_000), now)).toBe("12 s");
    expect(shortAge(ago(41 * 60_000), now)).toBe("41 min");
    expect(shortAge(ago(5 * 3_600_000), now)).toBe("5 h");
    expect(shortAge(ago(3 * 86_400_000), now)).toBe("3 d");
  });

  /** A clock skew must not print a negative age. */
  it("clamps a date in the future to zero", () => {
    expect(shortAge(new Date(now + 60_000).toISOString(), now)).toBe("0 s");
  });

  it("renders nothing rather than a placeholder that looks like data", () => {
    expect(shortAge(null)).toBeNull();
    expect(shortAge(undefined)).toBeNull();
    expect(shortAge("not a date")).toBeNull();
  });
});

describe("syncState (#802)", () => {
  const base = { fetching: false, fetchError: null, lastFetchAt: "2026-09-18T09:00:00Z" };

  it("reports the écart of the selected local branch", () => {
    expect(syncState({ ...base, branch: tracking("main", 0, 3) })).toEqual({
      kind: "gap",
      gap: { kind: "behind", behind: 3 },
      upstream: "origin/main",
      lastFetchAt: base.lastFetchAt,
    });
  });

  it("separates level, remote and no-upstream", () => {
    expect(syncState({ ...base, branch: tracking("main", 0, 0) }).kind).toBe("level");
    expect(syncState({ ...base, branch: remote("origin/main") }).kind).toBe("remote");
    expect(syncState({ ...base, branch: local("solo") }).kind).toBe("no-upstream");
    expect(syncState({ ...base, branch: undefined }).kind).toBe("no-upstream");
  });

  /**
   * While a fetch is in flight the numbers on screen are about to be replaced;
   * claiming "up to date" for a beat is the same stale reading the feature kills.
   */
  it("lets an in-flight fetch outrank everything", () => {
    expect(
      syncState({ ...base, fetching: true, branch: tracking("main", 0, 0) }).kind,
    ).toBe("fetching");
  });

  /**
   * A failed fetch dates every number on screen from the LAST SUCCESS, so it
   * outranks a `0↑ 0↓` that only means "nothing has been refreshed since".
   */
  it("lets a failed fetch outrank a stale zero gap", () => {
    const error = { kind: "network", message: "ssh: Could not resolve hostname" };
    const state = syncState({
      ...base,
      fetchError: error,
      branch: tracking("main", 0, 0),
    });
    expect(state).toEqual({ kind: "unknown", error, lastFetchAt: base.lastFetchAt });
  });
});

describe("upstreamSwitchTarget (#802)", () => {
  /**
   * The list DROPS `origin/main` while a local `main` exists (#571), so requiring
   * the target to be a listed row would kill the shortcut in exactly the case it
   * exists for. `source_branch` is posted verbatim and the daemon resolves
   * `refs/remotes/` too.
   */
  it("offers the upstream even though the list never carries it", () => {
    expect(upstreamSwitchTarget(tracking("main", 0, 3))).toBe("origin/main");
  });

  it("offers nothing when the branch tracks nothing", () => {
    expect(upstreamSwitchTarget(local("solo"))).toBeNull();
    expect(upstreamSwitchTarget(remote("origin/main"))).toBeNull();
    expect(upstreamSwitchTarget(undefined)).toBeNull();
  });
});

/**
 * #803/ADR-0070 §2 — what the LIST alone decides about the fast-forward.
 *
 * Optimistic at the click, named at the refusal: the two conditions that need a
 * working tree inspected are deliberately absent here, because checking them per
 * render is a race that always loses (a tree can get dirty between a render and a
 * click). They come back from the daemon's 409 instead.
 */
describe("canFastForward (#803)", () => {
  it("offers it on a branch strictly behind its upstream", () => {
    expect(canFastForward(tracking("main", 0, 3))).toBe(true);
  });

  it.each([
    ["diverged", tracking("main", 1, 3)],
    ["ahead only — the upstream has nothing to take", tracking("main", 2, 0)],
    ["level", tracking("main", 0, 0)],
    ["no upstream", local("solo")],
    ["a remote-tracking ref, which IS the upstream", remote("origin/main")],
    // A failed fetch leaves the counts unknowable: advancing onto a ref nobody
    // could refresh is the staleness the feature exists to remove.
    ["unknown after a failed fetch", local("main", { upstream: "origin/main" })],
  ])("offers nothing for %s", (_case, branch) => {
    expect(canFastForward(branch)).toBe(false);
  });

  it("offers nothing when there is no selection at all", () => {
    expect(canFastForward(undefined)).toBe(false);
  });
});

describe("fastForwardTouchesCheckout (#803)", () => {
  /**
   * The promise the popover makes before the click. Advancing HEAD walks the
   * working tree forward; advancing any other branch moves a ref and touches no
   * file — and both deserve saying, since silence about the first is a surprise
   * and silence about the second inherits the first's caution for nothing.
   */
  it("says the checkout moves for the checked-out branch", () => {
    expect(fastForwardTouchesCheckout(tracking("main", 0, 3, { head: true }))).toBe(true);
  });

  it("says no file changes for any other branch", () => {
    expect(fastForwardTouchesCheckout(tracking("main", 0, 3, { head: false }))).toBe(false);
  });

  it("promises neither when the list does not say", () => {
    expect(fastForwardTouchesCheckout(tracking("main", 0, 3))).toBeUndefined();
    expect(fastForwardTouchesCheckout(undefined)).toBeUndefined();
  });
});

describe("refusals (#803)", () => {
  const refusal = (reason: FastForwardRefusal["reason"]): FastForwardRefusal => ({
    reason,
    message: "",
  });

  /**
   * `up_to_date` and `no_upstream` can only arrive as a RACE — the button is never
   * shown for either. The refreshed list beside the 409 already tells the truth, so
   * an error card would paint a problem where the only event was a beat of latency.
   */
  it("treats the two races as benign", () => {
    expect(isBenignRefusal(refusal("up_to_date"))).toBe(true);
    expect(isBenignRefusal(refusal("no_upstream"))).toBe(true);
  });

  it("treats the two click-time refusals as real", () => {
    expect(isBenignRefusal(refusal("dirty_tree"))).toBe(false);
    expect(isBenignRefusal(refusal("checked_out_elsewhere"))).toBe(false);
  });

  /**
   * "Try again" only where the person can act on the cause. Retrying a branch
   * someone else's worktree holds changes nothing: the offer would be a loop.
   */
  it("offers a retry only for a dirty tree", () => {
    expect(isRetryableRefusal(refusal("dirty_tree"))).toBe(true);
    expect(isRetryableRefusal(refusal("checked_out_elsewhere"))).toBe(false);
    expect(isRetryableRefusal(refusal("diverged"))).toBe(false);
  });
});

describe("fastForwardLabel (#803)", () => {
  // The glossary says fast-forward and bans "pull": the button uses the git term
  // and names the branch, which is also what makes it unambiguous on a secondary.
  it("names the branch and the git verb", () => {
    expect(fastForwardLabel("main")).toBe("Fast-forward main");
  });

  it("elides a branch name too long for the button", () => {
    const label = fastForwardLabel("feature/a-very-long-branch-name");
    expect(label.startsWith("Fast-forward feature/")).toBe(true);
    expect(label.endsWith("…")).toBe(true);
  });
});
