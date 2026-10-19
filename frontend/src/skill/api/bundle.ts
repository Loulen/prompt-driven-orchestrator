/**
 * Skills of the bank as a zip: export a skill, a selection or folders, and
 * upload such a zip back into a bank (the pipeline sidecar's layout,
 * `<name>.skills/<id>/SKILL.md` + reference files).
 */

import { request } from "../../shared/http/client";
import { sidecarBlob } from "../../shared/http/sidecar";
import type { SkillSidecarImportReport } from "../../pipeline/api/document";

export interface SkillsExport {
  blob: Blob;
  /** The name the daemon gives the zip: the folder's, the skill's, or `skills`. */
  fileName: string;
  /** How many skills it carries. */
  count: number;
}

/** The file name of a `Content-Disposition`, its UTF-8 form first. */
export function attachmentFileName(disposition: string | null, fallback: string): string {
  if (!disposition) return fallback;
  const extended = /filename\*=UTF-8''([^;]+)/i.exec(disposition);
  if (extended) {
    try {
      return decodeURIComponent(extended[1].trim());
    } catch {
      // A malformed escape: the plain form below.
    }
  }
  const plain = /filename="([^"]*)"/i.exec(disposition);
  return plain?.[1] || fallback;
}

export async function exportSkills(body: { skill_ids: string[]; folder_ids: string[] }): Promise<SkillsExport> {
  const label = "POST /settings/skills/export";
  const resp = await request<Response>("POST", "/settings/skills/export", { body, responseMode: "raw", label });
  const blob = await sidecarBlob(resp, label);
  return {
    blob: blob ?? new Blob([], { type: "application/zip" }),
    fileName: attachmentFileName(resp.headers.get("content-disposition"), "selection.skills.zip"),
    count: Number(resp.headers.get("x-pdo-skills") ?? 0),
  };
}

/** Unknown ids are created under a root folder named after the file, known ids kept. */
export function uploadSkillsZip(file: File): Promise<SkillSidecarImportReport> {
  return request("POST", "/settings/skills/upload", {
    body: file,
    query: { name: file.name },
  });
}
