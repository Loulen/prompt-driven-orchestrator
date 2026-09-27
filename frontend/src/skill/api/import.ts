/** Import skills from a Source over HTTP (#670). */

import { request } from "../../shared/http/client";
import type { RecentSkillSource } from "../../shared/generated/RecentSkillSource";
import type { SkillImportItem } from "../../shared/generated/SkillImportItem";
import type { SkillImportReport } from "../../shared/generated/SkillImportReport";
import type { SkillRescanReport } from "../../shared/generated/SkillRescanReport";
import type { SkillScanResult } from "../model/skill";

/**
 * Clone shallow (or open a local folder) and list every `SKILL.md` with its
 * validity and collisions. Writes nothing to the bank. `scanId` is chosen by the
 * caller so the clone can be cancelled and then reused by `importSkills`.
 */
export function scanSkillSource(scanId: string, source: string): Promise<SkillScanResult> {
  return request("POST", "/settings/skills/scan", { body: { scan_id: scanId, source } });
}

export function cancelSkillScan(scanId: string): Promise<{ cancelled: boolean }> {
  return request("POST", `/settings/skills/scan/${encodeURIComponent(scanId)}/cancel`);
}

export function importSkills(body: {
  scan_id: string;
  source: string;
  folder?: { id?: string | null; name?: string | null; parent_id?: string | null };
  items: SkillImportItem[];
}): Promise<SkillImportReport> {
  return request("POST", "/settings/skills/import", { body });
}

export function fetchRecentSkillSources(): Promise<{ sources: RecentSkillSource[] }> {
  return request("GET", "/settings/skills/sources/recent");
}

/** Re-clone a Source folder's source and diff it against the folder. Read-only. */
export function rescanSkillFolder(folderId: string, scanId: string): Promise<SkillRescanReport> {
  return request("POST", `/settings/skill-folders/${encodeURIComponent(folderId)}/rescan`, {
    body: { scan_id: scanId },
  });
}

export function updateSkillFolderFromSource(
  folderId: string,
  body: { scan_id: string; items: { path: string; action: "update" | "import" }[] },
): Promise<SkillImportReport> {
  return request("POST", `/settings/skill-folders/${encodeURIComponent(folderId)}/update`, {
    body,
  });
}
