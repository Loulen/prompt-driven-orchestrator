/**
 * A skill's reference files over HTTP (#671). `path` may hold sub-folders
 * (`examples/login.spec.ts`); each segment is encoded, the `/` is kept for the
 * daemon's `{*path}` wildcard.
 */

import { request } from "../../shared/http/client";
import type { Skill } from "../../shared/generated/Skill";
import type { SkillFile } from "../../shared/generated/SkillFile";
import type { SkillFileContent, SkillFilesUpload } from "../model/skill";

function skillFilePath(id: string, path: string): string {
  const rel = path.split("/").map(encodeURIComponent).join("/");
  return `/settings/skills/${encodeURIComponent(id)}/files/${rel}`;
}

/**
 * Upload browser files (a drop or the file picker) as multipart. Each file
 * travels as a `path` text part (its destination, sub-folders kept) followed by
 * the `file` part. The daemon writes them in order and stops at the first
 * refusal, reporting what landed.
 */
export function uploadSkillFiles(
  id: string,
  files: { path: string; file: Blob }[],
): Promise<SkillFilesUpload> {
  const form = new FormData();
  for (const { path, file } of files) {
    form.append("path", path);
    form.append("file", file, path.split("/").pop() ?? path);
  }
  return request("POST", `/settings/skills/${encodeURIComponent(id)}/files`, { body: form });
}

/** The explorer path: the daemon copies `from_path` (absolute, on its host). */
export function uploadSkillFileFromPath(
  id: string,
  fromPath: string,
  path?: string,
): Promise<SkillFilesUpload> {
  return request("POST", `/settings/skills/${encodeURIComponent(id)}/files`, {
    body: { from_path: fromPath, path },
  });
}

export function fetchSkillFile(id: string, path: string): Promise<SkillFileContent> {
  return request("GET", skillFilePath(id, path));
}

/**
 * Save the editor's text. For `SKILL.md` the daemon re-runs the five checks and
 * answers the same named 400 as the paste popup; the response then carries the
 * refreshed `skill` row.
 */
export function writeSkillFile(
  id: string,
  path: string,
  text: string,
): Promise<SkillFile & { skill?: Skill }> {
  return request("PUT", skillFilePath(id, path), { body: new Blob([text], { type: "text/plain" }) });
}

export function deleteSkillFile(id: string, path: string): Promise<void> {
  return request("DELETE", skillFilePath(id, path), { responseMode: "void" });
}
