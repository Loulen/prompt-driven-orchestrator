/**
 * Hand-written shapes of the skill-bank payloads built with `json!` on the
 * daemon side (no Rust DTO yet).
 */

import type { ParsedSkillSource } from "../../shared/generated/ParsedSkillSource";
import type { Skill } from "../../shared/generated/Skill";
import type { SkillCandidate } from "../../shared/generated/SkillCandidate";
import type { SkillFile } from "../../shared/generated/SkillFile";
import type { SkillFolder } from "../../shared/generated/SkillFolder";

export interface SkillScanResult {
  scan_id: string;
  source: ParsedSkillSource;
  commit: string | null;
  candidates: SkillCandidate[];
  /** When the sub-path held nothing: folders elsewhere in the repo that do. */
  elsewhere: string[];
  elsewhere_count: number;
}

/** `GET /settings/skills`: the whole bank in one read. */
export interface SkillBank {
  skills: Skill[];
  folders: SkillFolder[];
  /** Where the skill folders live on disk (`<root>/<id>/SKILL.md`). */
  root_path: string;
}

/** `GET /settings/skills/{id}/files/{path}`: one file for the plain-text editor (#671). */
export interface SkillFileContent {
  path: string;
  size: number;
  /** `true` when the bytes are not UTF-8: `text` is `null` and the editor says so. */
  binary: boolean;
  text: string | null;
}

/** `POST /settings/skills/{id}/files`: what landed, and the whole list after. */
export interface SkillFilesUpload {
  uploaded: SkillFile[];
  files: SkillFile[];
}

/** `GET /settings/skills/{id}`: the row plus its content. */
export interface SkillDetail extends Skill {
  /** Raw `SKILL.md`; `null` if the folder vanished from disk. */
  content: string | null;
  frontmatter: Record<string, unknown> | null;
  /** Keys of `frontmatter` in the author's order (JSON objects do not keep it). */
  frontmatter_keys?: string[] | null;
  body: string | null;
  files: SkillFile[];
  path: string;
}

/** How many tiers select a skill: the instance, each Project, Trigger, pipeline node and Run. */
export function referentCount(r: SkillReferents): number {
  return (r.instance ? 1 : 0) + r.projects.length + (r.triggers?.length ?? 0) + r.pipelines.length + r.runs.length;
}

/** Who selects a skill, by tier (#669). */
export interface SkillReferents {
  skill_id: string;
  instance: boolean;
  projects: { id: string; name: string }[];
  triggers?: { id: string; name: string; pipeline_id: string }[];
  pipelines: { id: string; name: string; node_id?: string; scope?: string }[];
  runs: { run_id: string; name?: string | null; pipeline_name?: string | null }[];
}
