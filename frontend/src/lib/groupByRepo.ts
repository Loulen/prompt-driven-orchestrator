/**
 * "Group by project" for the Runs and Triggers lists (#258). Pure, deterministic,
 * client-side: the daemon only resolves each row's `effective_repo` (a concrete
 * path); the grouping itself is a reversible view layer.
 *
 * See CONTEXT.md § "Repo cible (`target_repo`)" for the domain rules this encodes.
 */

export interface RepoGroup<T> {
  /**
   * The full effective-repo path that keys this group. Empty string only for the
   * defensive nameless bucket (never produced by real daemon data, which always
   * resolves `effective_repo` to a concrete path).
   */
  repoPath: string;
  /**
   * Display label: the path's basename, disambiguated to a minimal trailing-path
   * suffix on basename collision (see {@link repoGroupLabel}). Empty for the
   * nameless bucket.
   */
  label: string;
  items: T[];
}

/** Path segments, dropping empty ones (leading `/`, doubled slashes, trailing `/`). */
function segments(path: string): string[] {
  return path.split("/").filter((s) => s.length > 0);
}

/** Basename of `path` (its last non-empty segment), or `path` itself if it has none. */
function lastSegment(path: string): string {
  const segs = segments(path);
  return segs.length ? segs[segs.length - 1] : path;
}

/** The last `k` segments of `path` joined by `/` (e.g. `lastKSegments("/a/b/c", 2) === "b/c"`). */
function lastKSegments(path: string, k: number): string {
  const segs = segments(path);
  return segs.slice(Math.max(0, segs.length - k)).join("/");
}

/**
 * The display label for an effective-repo path: its basename, or — when another
 * path in `allPaths` shares that basename — the minimal distinguishing
 * trailing-path suffix (e.g. `/a/foo` + `/b/foo` ⇒ `a/foo`). Falls back to the
 * full path if no trailing suffix can disambiguate (the full path is always
 * available via the header's `title`). See plan G6.
 */
export function repoGroupLabel(path: string, allPaths: string[]): string {
  const base = lastSegment(path);

  // Distinct paths sharing this basename. Dedupe (so several triggers on the same
  // repo don't force a useless suffix) and ensure `path` itself is considered.
  const colliding = [...new Set([path, ...allPaths])].filter(
    (p) => lastSegment(p) === base,
  );
  if (colliding.length <= 1) return base;

  const maxK = Math.max(...colliding.map((p) => segments(p).length));
  for (let k = 2; k <= maxK; k++) {
    const joins = colliding.map((p) => lastKSegments(p, k));
    if (new Set(joins).size === colliding.length) {
      return lastKSegments(path, k);
    }
  }
  return path; // safety fallback — never both bare basenames for a real collision.
}

/**
 * Group `items` by their effective repo path, preserving input order within each
 * group. Returns `null` when fewer than 2 distinct non-empty repos are present —
 * the caller then renders the flat list, byte-identical to the pre-#258
 * single-repo behavior.
 *
 * Groups are ordered alphabetically by full path (deterministic; groups don't
 * reshuffle as rows are added). Labels are basenames, disambiguated on collision.
 * An item whose `repoOf` is null/empty/undefined drops into a single nameless
 * bucket and does not count toward the "≥ 2 distinct repos" threshold.
 */
export function groupByRepo<T>(
  items: T[],
  repoOf: (item: T) => string | null | undefined,
): RepoGroup<T>[] | null {
  // First-seen key insertion order; within-group push order (Map preserves both).
  const buckets = new Map<string, T[]>();
  for (const item of items) {
    const raw = repoOf(item);
    const key = raw == null || raw.length === 0 ? "" : raw;
    const list = buckets.get(key);
    if (list) list.push(item);
    else buckets.set(key, [item]);
  }

  // Conditional grouping: need ≥ 2 distinct *non-empty* repos, else flat.
  const distinctRepos = [...buckets.keys()].filter((k) => k.length > 0);
  if (distinctRepos.length < 2) return null;

  // Lexicographic (code-unit) ordering — locale-independent and stable.
  const sortedKeys = [...buckets.keys()].sort((a, b) =>
    a < b ? -1 : a > b ? 1 : 0,
  );
  return sortedKeys.map((key) => ({
    repoPath: key,
    label: key.length > 0 ? repoGroupLabel(key, distinctRepos) : "",
    items: buckets.get(key)!,
  }));
}
