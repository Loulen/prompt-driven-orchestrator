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

/** A Projet reference: the identity the daemon returns, minimal for grouping. */
export interface ProjectRef {
  id: string;
  name: string;
}

/**
 * A group in the "group by Projet" view (#552). Either a **Projet** (several
 * member repo paths collapse under the human-given name) or a derived **path**
 * group (an effective repo in no Projet, labelled by basename as before, #258).
 */
export interface ProjectGroup<T> {
  /** Stable identity: `project:<id>` for a Projet, else the raw effective path. */
  key: string;
  kind: "project" | "path";
  /** The effective-repo path for a path group; empty for a Projet group. */
  repoPath: string;
  /** Display label: the Projet name, or the derived basename for a path group. */
  label: string;
  /** Hover title: the path (path group) or the joined member paths (Projet). */
  title: string;
  items: T[];
}

interface Bucket<T> {
  key: string;
  kind: "project" | "path";
  ref: ProjectRef | null;
  paths: Set<string>;
  items: T[];
}

/**
 * Group `items` by their **Projet** (#552, ADR-0046), falling back to the #258
 * per-path grouping for any repo in no Projet. The Projet of a path is resolved
 * **verbatim** by `projectOf` (a `path → ProjectRef | null` lookup the caller
 * builds from `GET /projects`); ADR-0033 forbids canonicalising, so two spellings
 * are two paths.
 *
 * Returns `null` — the flat list, byte-identical to no grouping — unless grouping
 * is warranted. Grouping shows when **there are ≥ 2 distinct groups**, OR **any
 * group is a named Projet**. The second clause is what makes a single Projet over
 * all of a list's repos still render under its name (the FP's "se rangent sous ce
 * nom"); the ≥ 2 clause preserves #258 exactly when no Projet exists — the "seuil
 * porte désormais sur les projets" of the AC, a Projet counting as one unit that
 * collapses its member paths.
 *
 * Groups are ordered by label (locale-independent code-unit compare), tie-broken
 * by key, so they never reshuffle as rows arrive. An item whose `repoOf` is
 * null/empty drops into a single nameless bucket that does not count toward the
 * threshold (defensive: real daemon data always resolves a concrete path).
 */
export function groupByProject<T>(
  items: T[],
  repoOf: (item: T) => string | null | undefined,
  projectOf: (path: string) => ProjectRef | null,
): ProjectGroup<T>[] | null {
  const buckets = new Map<string, Bucket<T>>();
  for (const item of items) {
    const raw = repoOf(item);
    const path = raw == null || raw.length === 0 ? "" : raw;
    const proj = path.length ? projectOf(path) : null;
    const key = proj ? `project:${proj.id}` : path;
    let bucket = buckets.get(key);
    if (!bucket) {
      bucket = {
        key,
        kind: proj ? "project" : "path",
        ref: proj,
        paths: new Set(),
        items: [],
      };
      buckets.set(key, bucket);
    }
    if (path.length) bucket.paths.add(path);
    bucket.items.push(item);
  }

  // Real buckets exclude the defensive nameless one (empty key).
  const real = [...buckets.values()].filter((b) => b.key.length > 0);
  const hasProject = real.some((b) => b.kind === "project");
  if (real.length < 2 && !hasProject) return null;

  // Path-group labels disambiguate among the path groups only (a Projet's paths
  // are hidden behind its name, so they never force a suffix on a path group).
  const pathKeys = real.filter((b) => b.kind === "path").map((b) => b.key);

  const groups: ProjectGroup<T>[] = [...buckets.values()].map((b) => {
    if (b.kind === "project") {
      return {
        key: b.key,
        kind: "project" as const,
        repoPath: "",
        label: b.ref ? b.ref.name : "",
        title: [...b.paths].sort().join(", "),
        items: b.items,
      };
    }
    return {
      key: b.key,
      kind: "path" as const,
      repoPath: b.key,
      label: b.key.length > 0 ? repoGroupLabel(b.key, pathKeys) : "",
      title: b.key,
      items: b.items,
    };
  });

  const cmp = (a: string, b: string) => (a < b ? -1 : a > b ? 1 : 0);
  groups.sort((a, b) => cmp(a.label, b.label) || cmp(a.key, b.key));
  return groups;
}
