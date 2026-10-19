import type { SkillBank } from "./skill";
import { sameRef, type TreeNodeRef, type TreeRow } from "./skillTree";
import { skillsInFolder } from "./skillSelection";

/**
 * The bank's multiple selection, for an export: a set of skill ids. Checking a
 * folder checks every skill under it at this instant (a gesture, as in a
 * selector); ⇧ checks the visible rows between the last one clicked and this one.
 */

/** The skill ids a row stands for: itself, or every skill under the folder. */
export function skillIdsOf(ref: TreeNodeRef, bank: Pick<SkillBank, "skills" | "folders">): string[] {
  if (ref.kind === "skill") return [ref.id];
  return skillsInFolder(ref.id, bank as SkillBank).map((skill) => skill.id);
}

/** `all` when every id is checked, `some`, or `none` (a folder without skill is `none`). */
export function checkState(checked: ReadonlySet<string>, ids: string[]): "all" | "some" | "none" {
  const hits = ids.filter((id) => checked.has(id)).length;
  if (ids.length > 0 && hits === ids.length) return "all";
  return hits > 0 ? "some" : "none";
}

/**
 * The selection after a click on `ref`'s box: its skills are checked, or
 * unchecked when all were. With ⇧ and a previous click (`anchor`) among the
 * `rows`, every row in between takes the same state.
 */
export function clickCheck(
  checked: ReadonlySet<string>,
  ref: TreeNodeRef,
  bank: Pick<SkillBank, "skills" | "folders">,
  range?: { anchor: TreeNodeRef | null; rows: TreeRow[] },
): Set<string> {
  const check = checkState(checked, skillIdsOf(ref, bank)) !== "all";
  let refs = [ref];
  if (range?.anchor) {
    const from = range.rows.findIndex((row) => sameRef(row.ref, range.anchor));
    const to = range.rows.findIndex((row) => sameRef(row.ref, ref));
    if (from >= 0 && to >= 0) {
      refs = range.rows.slice(Math.min(from, to), Math.max(from, to) + 1).map((row) => row.ref);
    }
  }
  const next = new Set(checked);
  for (const id of refs.flatMap((r) => skillIdsOf(r, bank))) {
    if (check) next.add(id);
    else next.delete(id);
  }
  return next;
}

/**
 * The export request of a selection: the folder when the selection is exactly
 * one folder's skills (the zip then carries its name, the deepest such folder's),
 * the skill ids otherwise.
 * Ids the bank lost are dropped.
 */
export function exportRequest(
  checked: ReadonlySet<string>,
  bank: Pick<SkillBank, "skills" | "folders">,
): { skill_ids: string[]; folder_ids: string[] } {
  const known = bank.skills.filter((skill) => checked.has(skill.id)).map((skill) => skill.id);
  // A parent holding only one sub-folder matches too: the deepest names the zip.
  const depth = (id: string | null): number => {
    let hops = 0;
    for (let cursor = id; cursor && hops < 100; hops += 1) {
      cursor = bank.folders.find((f) => f.id === cursor)?.parent_id ?? null;
    }
    return hops;
  };
  const folder = bank.folders
    .filter((candidate) => {
      const ids = skillIdsOf({ kind: "folder", id: candidate.id }, bank);
      return ids.length === known.length && ids.every((id) => checked.has(id));
    })
    .sort((a, b) => depth(b.id) - depth(a.id))[0];
  return folder && known.length > 0
    ? { skill_ids: [], folder_ids: [folder.id] }
    : { skill_ids: known, folder_ids: [] };
}
