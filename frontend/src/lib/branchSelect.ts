import type { BranchFetchError, BranchRef } from "../types";

/**
 * The branch a fresh repo seeds its source/base select with (#454, #571).
 *
 * Never a remote while any local exists: `main` local → `master` local → first
 * local → (no local at all) a remote ending in `/main` → `/master` → first
 * remote. Locality-aware so a repo whose default local is `master` can never
 * seed itself on `origin/main` — the class of bug the #454 rule was written to
 * kill. Returns `undefined` for an empty list.
 *
 * Shared by the primary select (`useLaunchTargets`) and every secondary row
 * (`SecondaryRepoRow`) so the two can never drift — this duplication was the
 * documented trap of #571.
 *
 * #802 leaves this rule ALONE on purpose (ADR-0070 §3): the default stays a
 * local branch even when it is behind. The écart is now visible, which is what
 * makes "you decide" a real offer instead of a shrug — but seeing it must not
 * rewrite the choice.
 */
export function pickDefaultBranch(list: BranchRef[]): string | undefined {
  const locals = list.filter((b) => b.kind === "local");
  const local =
    locals.find((b) => b.name === "main") ??
    locals.find((b) => b.name === "master") ??
    locals[0];
  if (local) return local.name;
  const remotes = list.filter((b) => b.kind === "remote");
  const remote =
    remotes.find((b) => b.name.endsWith("/main")) ??
    remotes.find((b) => b.name.endsWith("/master")) ??
    remotes[0];
  return remote?.name;
}

/**
 * The two groups the quick pick shows, in display order (#802).
 *
 * Locals first, then remotes; inside each group `main`/`master` lead, then the
 * rest by recency of their tip commit, then by name so the order is total (two
 * branches can share a commit, and an unordered list reshuffles under the user
 * on every refetch). A branch with no date sorts last: unknown is not recent.
 *
 * The daemon already partitions locals before remotes, but the ORDER INSIDE each
 * group is a display decision, so it lives here where the component tests can
 * pin it.
 */
export function groupBranches(list: BranchRef[]): {
  locals: BranchRef[];
  remotes: BranchRef[];
} {
  return {
    locals: sortGroup(list.filter((b) => b.kind === "local")),
    remotes: sortGroup(list.filter((b) => b.kind === "remote")),
  };
}

/** The bare branch name, remote prefix stripped — what `main`-first tests against. */
function bareName(name: string): string {
  const slash = name.indexOf("/");
  return slash === -1 ? name : name.slice(slash + 1);
}

function sortGroup(group: BranchRef[]): BranchRef[] {
  const rank = (b: BranchRef) => {
    const bare = bareName(b.name);
    if (bare === "main") return 0;
    if (bare === "master") return 1;
    return 2;
  };
  return [...group].sort((a, b) => {
    const byRank = rank(a) - rank(b);
    if (byRank !== 0) return byRank;
    const byRecency = commitTime(b) - commitTime(a);
    if (byRecency !== 0) return byRecency;
    return a.name.localeCompare(b.name);
  });
}

/** Tip-commit epoch, or `-Infinity` when the daemon reported no date (sorts last). */
function commitTime(b: BranchRef): number {
  if (!b.last_commit_at) return -Infinity;
  const at = Date.parse(b.last_commit_at);
  return Number.isNaN(at) ? -Infinity : at;
}

/**
 * Case-insensitive substring filter on the NAME only (#802).
 *
 * Not on the commit subject: typing `fix` would then surface every branch whose
 * last commit says "fix", which is most of them, and the list would stop being a
 * way to find a branch you can name. Order is preserved — a filter that re-ranks
 * makes the list jump under the keyboard cursor as characters are typed.
 */
export function filterBranches(list: BranchRef[], query: string): BranchRef[] {
  const needle = query.trim().toLowerCase();
  if (!needle) return list;
  return list.filter((b) => b.name.toLowerCase().includes(needle));
}

/**
 * Split `name` around the (case-insensitive) match of `query`, so the matched
 * run can be painted in the accent colour. Returns the whole name as a single
 * unmatched segment when there is no query or no match.
 */
export function highlightSegments(
  name: string,
  query: string,
): Array<{ text: string; match: boolean }> {
  const needle = query.trim().toLowerCase();
  if (!needle) return [{ text: name, match: false }];
  const at = name.toLowerCase().indexOf(needle);
  if (at === -1) return [{ text: name, match: false }];
  const segments = [
    { text: name.slice(0, at), match: false },
    { text: name.slice(at, at + needle.length), match: true },
    { text: name.slice(at + needle.length), match: false },
  ];
  return segments.filter((s) => s.text !== "");
}

/**
 * The écart amont of one branch, as the quick pick's chip shows it (#802).
 *
 * `null` means "this row shows no chip at all" — a remote-tracking ref (it IS
 * the upstream) and a local branch that tracks nothing. That is deliberately
 * different from `level` (`0↑ 0↓`, a real "up to date"): absence of a tracking
 * relation is not agreement with one.
 *
 * `unknown` is the fetch-failed state: the branch tracks something, but nobody
 * knows by how much any more.
 */
export type BranchGap =
  | { kind: "level" }
  | { kind: "unknown" }
  | { kind: "ahead"; ahead: number }
  | { kind: "behind"; behind: number }
  | { kind: "diverged"; ahead: number; behind: number };

export function branchGap(branch: BranchRef | undefined): BranchGap | null {
  if (!branch || branch.kind === "remote") return null;
  if (!branch.upstream) return null;
  const { ahead, behind } = branch;
  if (ahead == null || behind == null) return { kind: "unknown" };
  if (ahead === 0 && behind === 0) return { kind: "level" };
  if (ahead > 0 && behind > 0) return { kind: "diverged", ahead, behind };
  if (ahead > 0) return { kind: "ahead", ahead };
  return { kind: "behind", behind };
}

/** `3↑`, `2↓`, `3↑ 2↓` — the chip's text. Empty for the iconic states. */
export function gapLabel(gap: BranchGap | null): string {
  switch (gap?.kind) {
    case "ahead":
      return `${gap.ahead}↑`;
    case "behind":
      return `${gap.behind}↓`;
    case "diverged":
      return `${gap.ahead}↑ ${gap.behind}↓`;
    default:
      return "";
  }
}

/**
 * A short age, English units with a space (`12 s`, `41 min`, `5 h`, `3 d`).
 *
 * One formatter for the rows, the trigger and the popover sentences, so
 * "fetched 12 s ago" in the footer and `12 s` on a row can never disagree about
 * what "just now" means. Returns `null` for an absent or unparsable date — the
 * caller renders nothing rather than a placeholder that looks like data.
 */
export function shortAge(iso: string | null | undefined, now: number = Date.now()): string | null {
  if (!iso) return null;
  const at = Date.parse(iso);
  if (Number.isNaN(at)) return null;
  const seconds = Math.max(0, Math.round((now - at) / 1000));
  if (seconds < 60) return `${seconds} s`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes} min`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours} h`;
  return `${Math.floor(hours / 24)} d`;
}

/**
 * Everything the sync button and its popover show for the CURRENT selection
 * (#802). The one place the écart of the chosen branch is stated — the field
 * itself never repeats it (design decision, #800 grilling).
 *
 * `fetching` wins over every other state: while a fetch is in flight the numbers
 * on screen are about to be replaced, so claiming "up to date" for a beat would
 * be the same stale-reading bug the feature exists to kill.
 */
export type SyncState =
  | { kind: "fetching" }
  /** The last fetch failed: the écart is unknowable, launching is still fine. */
  | { kind: "unknown"; error: BranchFetchError; lastFetchAt: string | null }
  /** A remote-tracking ref is selected — it is fresh as of the last fetch. */
  | { kind: "remote"; lastFetchAt: string | null }
  /** A local branch that tracks nothing: there is no upstream to compare with. */
  | { kind: "no-upstream"; lastFetchAt: string | null }
  | { kind: "level"; upstream: string; lastFetchAt: string | null }
  | {
      kind: "gap";
      gap: Extract<BranchGap, { kind: "ahead" | "behind" | "diverged" }>;
      upstream: string;
      lastFetchAt: string | null;
    };

export function syncState({
  branch,
  fetching,
  fetchError,
  lastFetchAt,
}: {
  branch: BranchRef | undefined;
  fetching: boolean;
  fetchError: BranchFetchError | null;
  lastFetchAt: string | null;
}): SyncState {
  if (fetching) return { kind: "fetching" };
  // A failed fetch dates every number on screen from the LAST SUCCESS, so it
  // outranks whatever the refs currently say — including a `0↑ 0↓` that only
  // means "nothing has been refreshed since".
  if (fetchError) return { kind: "unknown", error: fetchError, lastFetchAt };
  if (!branch) return { kind: "no-upstream", lastFetchAt };
  if (branch.kind === "remote") return { kind: "remote", lastFetchAt };
  const gap = branchGap(branch);
  if (!gap || gap.kind === "unknown") return { kind: "no-upstream", lastFetchAt };
  const upstream = branch.upstream ?? "";
  if (gap.kind === "level") return { kind: "level", upstream, lastFetchAt };
  return { kind: "gap", gap, upstream, lastFetchAt };
}

/**
 * The "Launch from origin/x" shortcut's target, or `null` when the selected
 * branch tracks nothing (#802).
 *
 * The target is deliberately NOT required to be a row of the quick pick. The
 * daemon drops a remote-tracking ref whose bare name already exists as a local
 * branch (#571), so `origin/main` is exactly the ref that is never listed while
 * a local `main` exists — which is the commonest case this shortcut serves.
 * `source_branch` is posted verbatim and the daemon resolves it against
 * `refs/remotes/` too, so switching to an unlisted upstream launches correctly.
 *
 * This is NOT the #454 shows-one-sends-another trap: the quick pick's trigger
 * renders the held value verbatim (unlike a `<select>`, which silently falls
 * back to its first option), so what the field shows is still what launches.
 */
export function upstreamSwitchTarget(branch: BranchRef | undefined): string | null {
  return branch?.upstream ?? null;
}
