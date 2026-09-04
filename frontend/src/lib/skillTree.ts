import type { Skill, SkillFolder } from "../types";

/**
 * Pure helpers behind the bank's tree (#668). Kept out of the component so the
 * future tier selector can render the same rows with checkboxes instead of drag
 * handles (the design note: "same nodes, checkboxes instead of drag").
 */

export type TreeNodeRef = { kind: "folder"; id: string } | { kind: "skill"; id: string };

export interface TreeRow {
  ref: TreeNodeRef;
  depth: number;
  /** Folder: the folder; skill: the skill. */
  folder?: SkillFolder;
  skill?: Skill;
  /** Folder only: skills in it, recursively. */
  count?: number;
  /** Folder only: rendered expanded. */
  expanded?: boolean;
}

export function sameRef(a: TreeNodeRef | null, b: TreeNodeRef | null): boolean {
  return !!a && !!b && a.kind === b.kind && a.id === b.id;
}

const byName = <T extends { name: string; created_at: string }>(a: T, b: T) =>
  a.name.localeCompare(b.name, undefined, { sensitivity: "base" }) ||
  a.created_at.localeCompare(b.created_at);

/** `parent / child` breadcrumb of a folder, for pickers. */
export function folderPathLabel(folderId: string, folders: SkillFolder[]): string {
  const byId = new Map(folders.map((folder) => [folder.id, folder]));
  const parts: string[] = [];
  let cursor: string | null = folderId;
  let hops = 0;
  while (cursor && hops++ < 100) {
    const folder = byId.get(cursor);
    if (!folder) break;
    parts.unshift(folder.name);
    cursor = folder.parent_id;
  }
  return parts.join(" / ");
}

/** Ids of `folderId` and every folder below it. */
export function descendantFolderIds(folderId: string, folders: SkillFolder[]): Set<string> {
  const out = new Set<string>([folderId]);
  let grew = true;
  while (grew) {
    grew = false;
    for (const folder of folders) {
      if (folder.parent_id && out.has(folder.parent_id) && !out.has(folder.id)) {
        out.add(folder.id);
        grew = true;
      }
    }
  }
  return out;
}

/** Recursive skill count per folder. */
export function folderCounts(folders: SkillFolder[], skills: Skill[]): Map<string, number> {
  const counts = new Map<string, number>();
  const parentOf = new Map(folders.map((folder) => [folder.id, folder.parent_id]));
  for (const skill of skills) {
    let cursor = skill.folder_id;
    let hops = 0;
    while (cursor && hops++ < 100) {
      counts.set(cursor, (counts.get(cursor) ?? 0) + 1);
      cursor = parentOf.get(cursor) ?? null;
    }
  }
  return counts;
}

function matches(skill: Skill, needle: string): boolean {
  return (
    skill.name.toLowerCase().includes(needle) || skill.description.toLowerCase().includes(needle)
  );
}

/**
 * Flatten the bank into the rows the tree renders: folders first (sorted), then
 * skills, at every level. A non-empty `filter` keeps only matching skills plus
 * their ancestor folders, all expanded.
 */
export function buildRows(
  folders: SkillFolder[],
  skills: Skill[],
  expanded: ReadonlySet<string>,
  filter = "",
): TreeRow[] {
  const needle = filter.trim().toLowerCase();
  const counts = folderCounts(folders, skills);
  const visibleSkills = needle ? skills.filter((skill) => matches(skill, needle)) : skills;
  // Folders to show under a filter: every ancestor of a visible skill.
  let allowedFolders: Set<string> | null = null;
  if (needle) {
    allowedFolders = new Set();
    const parentOf = new Map(folders.map((folder) => [folder.id, folder.parent_id]));
    for (const skill of visibleSkills) {
      let cursor = skill.folder_id;
      let hops = 0;
      while (cursor && hops++ < 100) {
        allowedFolders.add(cursor);
        cursor = parentOf.get(cursor) ?? null;
      }
    }
  }

  const rows: TreeRow[] = [];
  const knownFolder = new Set(folders.map((folder) => folder.id));
  const walk = (parentId: string | null, depth: number) => {
    const children = folders
      .filter((folder) => (folder.parent_id ?? null) === parentId)
      .filter((folder) => !allowedFolders || allowedFolders.has(folder.id))
      .sort(byName);
    for (const folder of children) {
      const isExpanded = needle ? true : expanded.has(folder.id);
      rows.push({
        ref: { kind: "folder", id: folder.id },
        depth,
        folder,
        count: counts.get(folder.id) ?? 0,
        expanded: isExpanded,
      });
      if (isExpanded) walk(folder.id, depth + 1);
    }
    const here = visibleSkills
      .filter((skill) => {
        const parent = skill.folder_id && knownFolder.has(skill.folder_id) ? skill.folder_id : null;
        return parent === parentId;
      })
      .sort(byName);
    for (const skill of here) {
      rows.push({ ref: { kind: "skill", id: skill.id }, depth, skill });
    }
  };
  walk(null, 0);
  return rows;
}

/** Short id for the header: `7c3e…a91f`. */
export function shortId(id: string): string {
  return id.length > 12 ? `${id.slice(0, 4)}…${id.slice(-4)}` : id;
}
