/**
 * The rows of "Import skills from a source" (#670): what each found candidate
 * will do, and the summary the popup shows before Import. Pure: the popup's
 * state lives in `store/useImportSkills`.
 */

import type { RecentSkillSource } from "../../shared/generated/RecentSkillSource";
import type { SkillCandidate } from "../../shared/generated/SkillCandidate";

export type Step = "source" | "scanning" | "results" | "importing";

export type Resolution = "replace" | "rename" | "skip";

export interface RowState {
  checked: boolean;
  resolution: Resolution | null;
  renameTo: string;
}

export interface ScanError {
  code: string;
  message: string;
}

export const HOW_IT_WORKS = [
  "Point at a source",
  "Scan finds every SKILL.md",
  "Check the skills to import",
  "They land in a folder named after the source",
];

/** Rebuild the text a recent source was typed as, for the field. */
export function recentToText(source: RecentSkillSource): string {
  if (/^https?:\/\//.test(source.url) && (source.ref || source.path)) {
    return `${source.url}/tree/${source.ref ?? "main"}/${source.path}`.replace(/\/$/, "");
  }
  return source.url;
}

/** `git@github.com:owner/repo.git` for a GitHub-like https URL, for the callout. */
export function sshHint(url: string): string | null {
  const match = /^https?:\/\/([^/]+)\/([^/]+)\/([^/]+?)(?:\.git)?$/.exec(url);
  return match ? `git@${match[1]}:${match[2]}/${match[3]}.git` : null;
}

/** A checked row whose name is already in the bank must say replace / rename / skip. */
export function needsResolution(candidate: SkillCandidate): boolean {
  return candidate.status === "name_taken" || candidate.status === "same_commit";
}

export function defaultRow(candidate: SkillCandidate): RowState {
  return {
    checked: candidate.status === "new" || candidate.status === "name_taken",
    resolution: null,
    renameTo: "",
  };
}

/** The lower-cased new label of every checked `rename` row, by candidate path. */
export function renameTargetsOf(candidates: SkillCandidate[], rows: Map<string, RowState>): Map<string, string> {
  const out = new Map<string, string>();
  for (const candidate of candidates) {
    const row = rows.get(candidate.path);
    if (row?.checked && row.resolution === "rename" && row.renameTo.trim()) {
      out.set(candidate.path, row.renameTo.trim().toLowerCase());
    }
  }
  return out;
}

/**
 * Is `name` free for the candidate at `path`: not in the bank, not another
 * row's new label, not another checked new candidate's own name?
 */
export function isRenameFree(
  path: string,
  name: string,
  existingNames: string[],
  renameTargets: Map<string, string>,
  candidates: SkillCandidate[],
  rows: Map<string, RowState>,
): boolean {
  const lower = name.trim().toLowerCase();
  if (!lower) return false;
  if (existingNames.some((existing) => existing.toLowerCase() === lower)) return false;
  for (const [otherPath, other] of renameTargets) {
    if (otherPath !== path && other === lower) return false;
  }
  // Not another candidate's own (new) name either.
  return !candidates.some(
    (c) => c.path !== path && c.valid && c.status === "new" && rows.get(c.path)?.checked && c.name.toLowerCase() === lower,
  );
}

export interface ImportSummary {
  willImport: { candidate: SkillCandidate; as: string; action: "import" | "replace" | "rename" }[];
  unresolved: number;
  unchecked: number;
  invalid: number;
  sameCommit: number;
  files: number;
}

/** What Import will write, and what still blocks it. */
export function importSummary(
  candidates: SkillCandidate[],
  rows: Map<string, RowState>,
  doneRows: Set<string>,
  renameIsFree: (path: string, name: string) => boolean,
): ImportSummary {
  const willImport: ImportSummary["willImport"] = [];
  let unresolved = 0;
  let unchecked = 0;
  let files = 0;
  for (const candidate of candidates) {
    const row = rows.get(candidate.path);
    if (!row || !candidate.valid || doneRows.has(candidate.path)) continue;
    if (!row.checked) {
      // A same-commit duplicate is "already present", not "unchecked".
      if (candidate.status !== "same_commit") unchecked += 1;
      continue;
    }
    if (needsResolution(candidate)) {
      if (!row.resolution) {
        unresolved += 1;
        continue;
      }
      if (row.resolution === "skip") continue;
      if (row.resolution === "rename") {
        if (!renameIsFree(candidate.path, row.renameTo)) {
          unresolved += 1;
          continue;
        }
        willImport.push({ candidate, as: row.renameTo.trim(), action: "rename" });
      } else {
        willImport.push({ candidate, as: candidate.name, action: "replace" });
      }
    } else {
      willImport.push({ candidate, as: candidate.name, action: "import" });
    }
    files += candidate.file_count;
  }
  const invalid = candidates.filter((c) => !c.valid).length;
  const sameCommit = candidates.filter((c) => c.status === "same_commit").length;
  return { willImport, unresolved, unchecked, invalid, sameCommit, files };
}
