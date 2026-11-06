/**
 * The Espace level of the Runs list (#1041, CONTEXT.md « Espace »). Pure and
 * client-side: the daemon ships each Run's `space_id` (its own, or its root's for
 * a child Run) and the list of Espaces; this decides the first level of the list.
 * Inside each group the caller keeps the Projet / repo grouping of #552 / #258
 * unchanged.
 */
import type { Space } from "../types";

/** The key of the « Sans espace » ("No space") group. */
export const NO_SPACE_KEY = "space:none";

export interface SpaceGroup<T> {
  /** `space:<id>`, or {@link NO_SPACE_KEY}. */
  key: string;
  /** The Espace, or `null` for « Sans espace ». */
  space: Space | null;
  items: T[];
}

const cmp = (a: string, b: string) => (a < b ? -1 : a > b ? 1 : 0);

/** Espaces in display order: by name, case-insensitive, then by id (stable). */
export function sortSpaces(spaces: readonly Space[]): Space[] {
  return [...spaces].sort(
    (a, b) => cmp(a.name.toLowerCase(), b.name.toLowerCase()) || cmp(a.id, b.id),
  );
}

/**
 * Group `items` by Espace. Returns `null` when the instance has NO Espace at all:
 * the caller then renders the list exactly as before #1041 — no « Sans espace »
 * header, so a single-user instance sees no change.
 *
 * Otherwise, one group per Espace (by name), EVEN AN EMPTY ONE — its header is
 * where a Run is dropped and where the Espace is edited — then « Sans espace »,
 * always last and always present (it is the drop target that takes a Run out of
 * an Espace). An item naming an Espace that no longer exists (deleted in another
 * browser, list not yet refreshed) falls in « Sans espace », as the daemon will
 * have it. Input order is preserved inside each group.
 */
export function groupBySpace<T>(
  items: readonly T[],
  spaceIdOf: (item: T) => string | null | undefined,
  spaces: readonly Space[],
): SpaceGroup<T>[] | null {
  if (spaces.length === 0) return null;
  const groups = sortSpaces(spaces).map<SpaceGroup<T>>((space) => ({
    key: `space:${space.id}`,
    space,
    items: [],
  }));
  const byId = new Map(groups.map((g) => [g.space!.id, g]));
  const none: SpaceGroup<T> = { key: NO_SPACE_KEY, space: null, items: [] };
  for (const item of items) {
    const id = spaceIdOf(item);
    (id ? byId.get(id) ?? none : none).items.push(item);
  }
  return [...groups, none];
}

/** `id → Espace` lookup for row marks (stripe, archived swatch). */
export function spaceLookup(spaces: readonly Space[]): (id: string | null | undefined) => Space | null {
  const map = new Map(spaces.map((s) => [s.id, s]));
  return (id) => (id ? map.get(id) ?? null : null);
}
