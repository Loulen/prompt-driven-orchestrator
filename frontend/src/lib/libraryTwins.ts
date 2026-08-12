/**
 * The Library "twin" rule (#227) — one source of truth.
 *
 * A working pipeline and its durable Library copy are linked by NAME, never by
 * id: the Library copy's id is an independently derived slug that can diverge
 * from the working pipeline's file-stem id (#227, commit 51b5ee1). Every star,
 * cascade and library-only decision in the left panel joins on that one key.
 *
 * These predicates back a SAFETY decision — a cascade delete destroys a second
 * file the user never pointed at — so they live here instead of inline at each
 * call site. The delete's EXECUTION path (`handleConfirmDelete`) and the
 * checkbox's VISIBILITY path (the `ConfirmDeleteModal` render) used to compute
 * the rule separately; they now agree by construction rather than by two copies
 * staying in step.
 */
import type { LibraryPipelineEntry } from "../api";
import type { PipelineListEntry } from "../types";

/**
 * The one join key: same display name. Never `id` — see the module note. Every
 * predicate below routes through this, so the key exists in exactly one place.
 */
function sameName(a: { name: string }, b: { name: string }): boolean {
  return a.name === b.name;
}

/**
 * Every Library entry twinned with `entry`. Usually 0 or 1; a "double-star" (the
 * same name saved in both the repo-scoped and user-scoped library) yields 2+,
 * which is the ambiguous case {@link cascadableTwin} refuses to act on.
 */
export function libraryTwins(
  entry: { name: string },
  library: LibraryPipelineEntry[],
): LibraryPipelineEntry[] {
  return library.filter((lp) => sameName(lp, entry));
}

/**
 * The Library copy a delete of `target` may cascade to, or `null` when no
 * cascade is on offer. Null-safe on `target` so the checkbox-visibility site can
 * ask before any row is picked (`deleteTarget === null` ⇒ no cascade label).
 *
 * Two guards, both load-bearing:
 *   - `scope !== "library"`: the row already IS the library entry, so there is
 *     no separate copy left to remove — the plain delete covers it (#216).
 *   - exactly one twin: a double-star is ambiguous, and guessing which copy to
 *     destroy is exactly the mistake a cascade must never make.
 */
export function cascadableTwin(
  target: Pick<PipelineListEntry, "name" | "scope"> | null,
  library: LibraryPipelineEntry[],
): LibraryPipelineEntry | null {
  if (target == null) return null;
  if (target.scope === "library") return null;
  const twins = libraryTwins(target, library);
  return twins.length === 1 ? twins[0] : null;
}

/**
 * Library entries with no same-name row in `pipelines`. These are the ones the
 * merged /pipelines list doesn't already surface, so the panel renders them as
 * their own passive rows — starring a brand-new pipeline yields a visible
 * sidebar entry, matching "starred == in the library".
 */
export function libraryOnly(
  library: LibraryPipelineEntry[],
  pipelines: { name: string }[],
): LibraryPipelineEntry[] {
  return library.filter((lp) => !pipelines.some((p) => sameName(p, lp)));
}

/**
 * Whether `pipeline` shows the star badge: a Library entry exists under the same
 * name. This is the visible confirmation the user expects after clicking the
 * canvas star.
 */
export function isStarred(
  pipeline: { name: string },
  library: LibraryPipelineEntry[],
): boolean {
  return libraryTwins(pipeline, library).length > 0;
}
