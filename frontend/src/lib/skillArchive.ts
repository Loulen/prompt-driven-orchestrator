import type { Skill, SkillArchiveFormat, SkillFolder } from "../types";
import { descendantFolderIds } from "./skillTree";

/**
 * Pure helpers behind the **Archive de skills** export (pdo-issues#2, #1026,
 * ADR-0082). The daemon applies the same file-name rule (`.zip` appended, one
 * skill → `<name>.skills.zip`, else `pdo-skills-<YYYYMMDD-HHMM>.zip`); the
 * modal pre-fills with it, plus one UI refinement: a selection that is exactly
 * one folder's skills is named after that folder.
 */

/** Seeded skills (« Managed by PDO ») never enter an archive. */
export function isExportable(skill: Pick<Skill, "locked">): boolean {
  return !(skill.locked ?? false);
}

/** Characters a file name never carries across systems. */
// eslint-disable-next-line no-control-regex
const FORBIDDEN = /[\u0000-\u001f\u007f/\\:*?"<>|]/g;

/** A name made safe as an archive / folder name, as the daemon does. */
export function sanitiseArchiveName(name: string): string {
  const replaced = name.replace(FORBIDDEN, "-").replace(/^[\s.]+|[\s.]+$/g, "");
  return replaced === "" ? "skill" : replaced;
}

/** `YYYYMMDD-HHMM`, local time. */
export function archiveStamp(now: Date): string {
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${now.getFullYear()}${pad(now.getMonth() + 1)}${pad(now.getDate())}-${pad(now.getHours())}${pad(now.getMinutes())}`;
}

/** `.zip` appended when missing (case-insensitive). */
export function withZip(name: string): string {
  const trimmed = name.trim();
  if (trimmed === "") return "";
  return /\.zip$/i.test(trimmed) ? trimmed : `${trimmed}.zip`;
}

/** Ids of every exportable skill under `folderId`, sub-folders included. */
export function exportableInFolder(folderId: string, folders: SkillFolder[], skills: Skill[]): string[] {
  const scope = descendantFolderIds(folderId, folders);
  return skills
    .filter((skill) => skill.folder_id != null && scope.has(skill.folder_id) && isExportable(skill))
    .map((skill) => skill.id);
}

/**
 * The folder whose exportable skills are exactly `checked` — the outermost one
 * when several nested folders qualify — or `null`.
 */
export function folderOfSelection(
  checked: ReadonlySet<string>,
  folders: SkillFolder[],
  skills: Skill[],
): SkillFolder | null {
  if (checked.size === 0) return null;
  const parentOf = new Map(folders.map((folder) => [folder.id, folder.parent_id]));
  const depth = (folder: SkillFolder) => {
    let d = 0;
    let cursor = folder.parent_id;
    while (cursor && d < 100) {
      d += 1;
      cursor = parentOf.get(cursor) ?? null;
    }
    return d;
  };
  const matching = folders.filter((folder) => {
    const ids = exportableInFolder(folder.id, folders, skills);
    return ids.length === checked.size && ids.every((id) => checked.has(id));
  });
  if (matching.length === 0) return null;
  return matching.reduce((best, folder) => (depth(folder) < depth(best) ? folder : best));
}

/**
 * The file name the modal proposes for a selection: `<skill>.skills.zip` for
 * one skill, `<folder>.skills.zip` when the selection is exactly one folder,
 * `pdo-skills-<YYYYMMDD-HHMM>.zip` otherwise.
 */
export function defaultArchiveName(
  checked: ReadonlySet<string>,
  folders: SkillFolder[],
  skills: Skill[],
  now: Date,
): string {
  if (checked.size === 1) {
    const [id] = [...checked];
    const skill = skills.find((s) => s.id === id);
    if (skill) return `${sanitiseArchiveName(skill.name)}.skills.zip`;
  }
  const folder = folderOfSelection(checked, folders, skills);
  if (folder) return `${sanitiseArchiveName(folder.name)}.skills.zip`;
  return `pdo-skills-${archiveStamp(now)}.zip`;
}

// ---- Import d'archive (#1027) ----------------------------------------------

/** Archive-level ceiling, mirrored from the daemon (`skill_archive::MAX_ARCHIVE_BYTES`). */
export const MAX_ARCHIVE_BYTES = 64 * 1024 * 1024;

/** The chip next to the file name on the results' source line. */
export const ARCHIVE_FORMAT_LABEL: Record<SkillArchiveFormat, string> = {
  pdo_archive: "PDO archive · ids kept",
  pipeline_sidecar: "pipeline sidecar · ids kept",
  plain_zip: "zip without manifest · by name",
};

/**
 * Why a dropped / picked file is refused in place before any upload, or `null`
 * when it may go to the daemon (which has the last word: corrupted, no
 * `SKILL.md`…). Only one `.zip` at a time.
 */
export function refuseArchive(files: ArrayLike<File>): string | null {
  if (files.length === 0) return null;
  if (files.length > 1) return `${files.length} files dropped: drop one .zip at a time.`;
  const file = files[0];
  if (!/\.zip$/i.test(file.name)) return `${file.name} is not a .zip.`;
  if (file.size > MAX_ARCHIVE_BYTES) {
    return `${file.name} is ${(file.size / (1024 * 1024)).toFixed(1)} MB, over the ${MAX_ARCHIVE_BYTES / (1024 * 1024)} MB limit.`;
  }
  return null;
}

/** `team-review.skills.zip` → `team-review` (mirror of `skill_archive::folder_stem`). */
export function archiveFolderStem(fileName: string): string {
  const last = (fileName.split(/[/\\]/).pop() ?? fileName).trim();
  const stem = last.replace(/\.zip$/i, "").replace(/\.skills$/i, "").trim();
  return stem === "" ? "archive" : stem;
}

/** `stem`, else `stem 2`, `stem 3`… — the first name none of `taken` carries. */
export function freeFolderName(stem: string, taken: string[]): string {
  const lower = new Set(taken.map((name) => name.trim().toLowerCase()));
  if (!lower.has(stem.toLowerCase())) return stem;
  for (let n = 2; ; n += 1) {
    const candidate = `${stem} ${n}`;
    if (!lower.has(candidate.toLowerCase())) return candidate;
  }
}

/**
 * Where an archive's created skills land under `parentId`: the folder already
 * named after the file there (offered for **merge**), and the free name the
 * **New folder** choice is pre-filled with. At the root it matches what the
 * daemon's scan reports.
 */
export function archiveDestination(
  stem: string,
  folders: SkillFolder[],
  parentId: string | null,
): { existing: SkillFolder | null; newName: string } {
  const siblings = folders.filter((folder) => (folder.parent_id ?? null) === parentId);
  const existing = siblings.find((folder) => folder.name.trim().toLowerCase() === stem.toLowerCase()) ?? null;
  return {
    existing,
    newName: freeFolderName(
      stem,
      siblings.map((folder) => folder.name),
    ),
  };
}
