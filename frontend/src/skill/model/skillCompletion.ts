import type { Skill } from "../../shared/generated/Skill";
import type { SkillFolder } from "../../shared/generated/SkillFolder";
import { activeQuery, type ActiveQuery } from "../../shared/extensions/completion";

/**
 * The completion of skills in a prompt (a node's, New Run's): `/` at the start
 * of a word opens the bank's skills, those the node (or the Run) already
 * receives first, filtered by what follows the `/`. Picking one inserts
 * `/<name>`; one it does not receive yet is added to its selection by the
 * caller.
 */

export const SKILL_TRIGGER = "/";

/** What a skill name may hold while it is typed: anything else closes the list. */
const NAME_CHARS = /^[A-Za-z0-9._-]*$/;

/**
 * The `/` query the caret is in, or `null`. The `/` opens only at the start of
 * a word (`a/b`, `https://…` do not), and a second `/` closes it (`/usr/bin`
 * is a path, not a skill).
 */
export function skillQuery(text: string, caret: number): ActiveQuery | null {
  const query = activeQuery(text, caret, [SKILL_TRIGGER]);
  if (!query || !NAME_CHARS.test(query.query)) return null;
  return query;
}

/** One row of the list. */
export interface SkillOption {
  skill: Skill;
  /** The node (or the Run) already receives it, from any tier. */
  received: boolean;
  /** `Folder/Sub/` of the skill in the bank, `""` at the root. */
  folder: string;
}

/**
 * The skills of the bank matching `query` (a part of the name, any case):
 * the received ones first, then the rest of the bank; within each group, the
 * names that start with the query first, then by name.
 */
export function skillOptions(
  bank: { skills: Skill[]; folders: SkillFolder[] },
  received: ReadonlySet<string>,
  query: string,
): SkillOption[] {
  const needle = query.toLowerCase();
  const folderPath = folderPaths(bank.folders);
  const rank = (skill: Skill) =>
    (received.has(skill.id) ? 0 : 2) + (skill.name.toLowerCase().startsWith(needle) ? 0 : 1);
  return bank.skills
    .filter((skill) => skill.name.toLowerCase().includes(needle))
    .sort((a, b) => rank(a) - rank(b) || a.name.localeCompare(b.name))
    .map((skill) => ({
      skill,
      received: received.has(skill.id),
      folder: skill.folder_id ? folderPath(skill.folder_id) : "",
    }));
}

function folderPaths(folders: SkillFolder[]): (id: string) => string {
  const byId = new Map(folders.map((folder) => [folder.id, folder]));
  return (id) => {
    const parts: string[] = [];
    let cursor: string | null = id;
    for (let hops = 0; cursor && hops < 100; hops += 1) {
      const folder = byId.get(cursor);
      if (!folder) break;
      parts.unshift(folder.name);
      cursor = folder.parent_id;
    }
    return parts.length > 0 ? `${parts.join("/")}/` : "";
  };
}
