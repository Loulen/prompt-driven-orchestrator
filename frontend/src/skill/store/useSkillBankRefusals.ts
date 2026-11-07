import { useCallback, useMemo } from "react";
import type { OperationQuestion, OperationVerb } from "../../shared/authorization/api";
import { useRefusals } from "../../shared/authorization/useRefusals";
import type { Skill } from "../../shared/generated/Skill";
import type { SkillFolder } from "../../shared/generated/SkillFolder";

/**
 * Which gestures of the bank the caller may not make, and why (CORE-205):
 * one question for the whole screen, each refused gesture greyed with the
 * refusal's sentence. `undefined` = allowed (or not answered yet).
 */
export interface SkillBankRefusals {
  /** Paste a SKILL.md (`skill` × `create`). */
  createSkill?: string;
  /** New folder / subfolder (`skill_folder` × `create`). */
  createFolder?: string;
  /** Import from a source: the scan, then the import. */
  importFromSource?: string;
  /** Upload a zip (`skill` × `import`). */
  uploadZip?: string;
  /** Rename, move, edit / add / delete its files (`skill` × `update`). */
  updateSkill: (id: string) => string | undefined;
  deleteSkill: (id: string) => string | undefined;
  /** Rename, move (`skill_folder` × `update`). */
  updateFolder: (id: string) => string | undefined;
  deleteFolder: (id: string) => string | undefined;
  /** Update from source: the re-scan, then the update. */
  updateFolderFromSource: (id: string) => string | undefined;
}

const SKILL = "skill";
const FOLDER = "skill_folder";
const action = (name: string): OperationVerb => ({ action: name });

/** The global Operations, in the order of the answer. */
const GLOBAL: OperationQuestion[] = [
  { resource: SKILL, verb: "create" },
  { resource: FOLDER, verb: "create" },
  { resource: SKILL, verb: action("scan") },
  { resource: SKILL, verb: action("import") },
];
const SKILL_VERBS: OperationVerb[] = ["update", "delete"];
const FOLDER_VERBS: OperationVerb[] = ["update", "delete", action("rescan"), action("update_from_source")];

export function useSkillBankRefusals(skills: Skill[], folders: SkillFolder[]): SkillBankRefusals {
  // A locked skill refuses every write on its own: no need to ask.
  const skillIds = useMemo(() => skills.filter((s) => !s.locked).map((s) => s.id), [skills]);
  const folderIds = useMemo(() => folders.map((f) => f.id), [folders]);
  const operations = useMemo<OperationQuestion[]>(
    () => [
      ...GLOBAL,
      ...skillIds.flatMap((id) => SKILL_VERBS.map((verb) => ({ resource: SKILL, verb, target: id }))),
      ...folderIds.flatMap((id) => FOLDER_VERBS.map((verb) => ({ resource: FOLDER, verb, target: id }))),
    ],
    [skillIds, folderIds],
  );
  const refusals = useRefusals(operations);

  const skillAt = useMemo(() => new Map(skillIds.map((id, i) => [id, GLOBAL.length + i * SKILL_VERBS.length])), [skillIds]);
  const folderAt = useMemo(
    () => new Map(folderIds.map((id, i) => [id, GLOBAL.length + skillIds.length * SKILL_VERBS.length + i * FOLDER_VERBS.length])),
    [folderIds, skillIds.length],
  );
  const ofSkill = useCallback(
    (id: string, offset: number) => {
      const at = skillAt.get(id);
      return at === undefined ? undefined : refusals[at + offset];
    },
    [skillAt, refusals],
  );
  const ofFolder = useCallback(
    (id: string, offset: number) => {
      const at = folderAt.get(id);
      return at === undefined ? undefined : refusals[at + offset];
    },
    [folderAt, refusals],
  );

  return useMemo(
    () => ({
      createSkill: refusals[0],
      createFolder: refusals[1],
      importFromSource: refusals[2] ?? refusals[3],
      uploadZip: refusals[3],
      updateSkill: (id) => ofSkill(id, 0),
      deleteSkill: (id) => ofSkill(id, 1),
      updateFolder: (id) => ofFolder(id, 0),
      deleteFolder: (id) => ofFolder(id, 1),
      updateFolderFromSource: (id) => ofFolder(id, 2) ?? ofFolder(id, 3),
    }),
    [refusals, ofSkill, ofFolder],
  );
}
