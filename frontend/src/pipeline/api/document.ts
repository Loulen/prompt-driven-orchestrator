/**
 * The portable pipeline document (ADR-0059) over HTTP: its export, its skills
 * sidecar (#673), and its import.
 */

import type { SkillRef } from "../../shared/generated/SkillRef";
import { request } from "../../shared/http/client";
import { sidecarBlob } from "../../shared/http/sidecar";

export function fetchPipelineDocument(id: string): Promise<string> {
  return request("GET", `/pipelines/${encodeURIComponent(id)}/document`, {
    responseMode: "text",
    label: `GET /pipelines/${id}/document`,
  });
}

/// The skills sidecar of a pipeline's portable document (#673 / ADR-0062): a zip
/// of `<pipeline>.skills/<id>/…` to unpack next to the YAML. `null` when the
/// pipeline references no skill (the daemon answers 204: the YAML is the whole
/// document).
export async function fetchPipelineSkillsSidecar(id: string): Promise<Blob | null> {
  const resp = await request<Response>(
    "GET",
    `/pipelines/${encodeURIComponent(id)}/document/skills`,
    { responseMode: "raw", label: `GET /pipelines/${id}/document/skills` },
  );
  return sidecarBlob(resp, `GET /pipelines/${id}/document/skills`);
}

/// What the import did to the Banque de skills (#673): created ids (same ids as
/// the document), ids the bank already knew (untouched), labels that had to be
/// suffixed, and ids found neither in the bank nor in the sidecar.
export interface SkillSidecarImportReport {
  created: SkillRef[];
  kept: SkillRef[];
  renamed: { id: string; from: string; to: string }[];
  missing: SkillRef[];
  folder?: { id: string; name: string };
  warnings: string[];
}

export interface ImportPipelineDocumentResult {
  id: string;
  scope: string;
  path: string;
  /// Non-fatal diagnostics: prompts dropped because they name a node the
  /// document does not define, and the skills the import renamed or could not
  /// find (#673).
  warnings: string[];
  skills?: SkillSidecarImportReport;
}

/// `skillsSidecar` is the base64 of the sidecar zip PDO exported (or of the
/// `<pipeline>.skills/` folder re-zipped). Without it, unknown skill ids import
/// with a "skill absent" warning — never a failure.
export function importPipelineDocument(
  document: string,
  skillsSidecar?: string,
): Promise<ImportPipelineDocumentResult> {
  return request("POST", "/pipelines/import", {
    body: skillsSidecar ? { document, skills_sidecar: skillsSidecar } : { document },
    label: "POST /pipelines/import",
  });
}
