/**
 * The Banque de skills over HTTP (#668, ADR-0062). Separate REST resources
 * under `/settings`, NOT part of the grouped `PUT /settings`: a skill is a ROW
 * plus a folder on disk. Every gesture commits immediately (no unsaved state in
 * the bank).
 */

import { request } from "../../shared/http/client";
import type { Skill } from "../../shared/generated/Skill";
import type { SkillFolder } from "../../shared/generated/SkillFolder";
import type { SkillBank, SkillDetail, SkillReferents } from "../model/skill";

export function fetchSkillBank(): Promise<SkillBank> {
  return request("GET", "/settings/skills");
}

export function createSkill(body: {
  content: string;
  name?: string;
  folder_id?: string | null;
}): Promise<Skill> {
  return request("POST", "/settings/skills", { body });
}

export function fetchSkill(id: string): Promise<SkillDetail> {
  return request("GET", `/settings/skills/${encodeURIComponent(id)}`);
}

/** Sparse edit: `name` renames the label only; `folder_id: null` moves to the root. */
export function updateSkill(
  id: string,
  patch: { name?: string; folder_id?: string | null },
): Promise<Skill> {
  return request("PUT", `/settings/skills/${encodeURIComponent(id)}`, { body: patch });
}

export function deleteSkill(id: string): Promise<void> {
  return request("DELETE", `/settings/skills/${encodeURIComponent(id)}`, {
    responseMode: "void",
  });
}

export function fetchSkillReferents(id: string): Promise<SkillReferents> {
  return request("GET", `/settings/skills/${encodeURIComponent(id)}/referents`);
}

export function createSkillFolder(body: {
  name: string;
  parent_id?: string | null;
}): Promise<SkillFolder> {
  return request("POST", "/settings/skill-folders", { body });
}

export function updateSkillFolder(
  id: string,
  patch: { name?: string; parent_id?: string | null },
): Promise<SkillFolder> {
  return request("PUT", `/settings/skill-folders/${encodeURIComponent(id)}`, { body: patch });
}

/** The folder's skills and sub-folders move to its parent; no skill is deleted. */
export function deleteSkillFolder(id: string): Promise<void> {
  return request("DELETE", `/settings/skill-folders/${encodeURIComponent(id)}`, {
    responseMode: "void",
  });
}
