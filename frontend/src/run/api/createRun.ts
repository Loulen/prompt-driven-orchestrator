/**
 * Creating a Run (`POST /runs`, JSON or multipart with attachments) and editing
 * its repositories mid-run (`PATCH /runs/{id}/repos`).
 */

import type { RunState } from "../../shared/generated/RunState";
import type { AgentChoice } from "../../shared/generated/AgentChoice";
import type { ProvisioningRules } from "../../shared/generated/ProvisioningRules";
import type { SkillRef } from "../../shared/generated/SkillRef";
import { apiErrorMessage, request } from "../../shared/http/client";

/** One repo line of a multi-repo create (#465, ADR-0042/0047): a path, an optional
 *  base branch (default HEAD, the local ref), and an optional `read_only` opt-in
 *  (default `false` ⇒ writable). `[0]` is the primary; `[1..]` become secondary
 *  snapshots, writable unless `read_only`. Shared by create, `EditRunReposBody.add`
 *  and the trigger `target_repos` blob. */
export interface TargetRepoInput {
  repo: string;
  base_branch?: string;
  read_only?: boolean;
}

export interface CreateRunRequest {
  pipeline: string;
  input: string;
  variables: Record<string, unknown>;
  pipeline_id?: string;
  target_repo?: string;
  /** ADR-0087: the Projet the Run is launched in; omitted for none. */
  project_id?: string;
  /** Read-only secondary repos (#465). `[0]` is the primary (kept in sync with
   *  `target_repo`), `[1..]` are secondaries. Omit for a mono-repo Run. */
  target_repos?: TargetRepoInput[];
  source_branch?: string;
  name?: string;
  /** Explicit sandbox (#410/#432): `"off"` or a staging-profile name. Omitted → the
   *  server defers to the trigger/instance default at the create chokepoint. */
  sandbox?: string;
  /** Explicit harness (#551, ADR-0046): the `run` tier of the precedence chain. Omitted/
   *  blank → the Run names no harness and each free node resolves through the instance
   *  default and the `claude` floor. Frozen into `RunStarted` at the create chokepoint. */
  harness?: string;
  agent_choice?: AgentChoice;
  /** #669/ADR-0062: the Run tier of the skills selection, frozen into `RunStarted`
   *  when non-empty. Omit for none. */
  skills?: SkillRef[];
  /** Whether the manager auto-names this Run (#338). The modal always sends it; omit and
   *  the server resolves back-compat by the presence of `name`, then the instance default. */
  auto_name?: boolean;
  images?: File[];
  /** #779: non-image attachments, sent as the multipart `files` field. */
  files?: File[];
  provisioning?: ProvisioningRules;
  /** API 0.9: extension id → the value the Run carries for it (the `run/new`
   *  sections' `provide`). Each extension checks its own before the Run
   *  exists; an id no installed extension has is refused. Omit for none. */
  extensions?: Record<string, unknown>;
}

export interface CreateRunResponse {
  run_id: string;
}

export function createRun(req: CreateRunRequest): Promise<CreateRunResponse> {
  const hasImages = req.images && req.images.length > 0;
  const hasFiles = req.files && req.files.length > 0;

  if (hasImages || hasFiles) {
    const form = new FormData();
    form.append("pipeline", req.pipeline);
    form.append("input", req.input);
    form.append("variables", JSON.stringify(req.variables));
    if (req.pipeline_id) form.append("pipeline_id", req.pipeline_id);
    if (req.target_repo) form.append("target_repo", req.target_repo);
    if (req.project_id) form.append("project_id", req.project_id);
    // Every field of the JSON path must be mirrored here: a field the multipart
    // branch omits is silently dropped for a Run created WITH attached images.
    // Non-scalars ride as JSON strings, bools as stringified bools.
    if (req.target_repos && req.target_repos.length > 0)
      form.append("target_repos", JSON.stringify(req.target_repos));
    if (req.source_branch) form.append("source_branch", req.source_branch);
    if (req.name) form.append("name", req.name);
    if (req.sandbox) form.append("sandbox", req.sandbox);
    if (req.harness) form.append("harness", req.harness);
    if (req.agent_choice) form.append("agent_choice", JSON.stringify(req.agent_choice));
    if (req.skills && req.skills.length > 0) form.append("skills", JSON.stringify(req.skills));
    if (req.auto_name !== undefined) form.append("auto_name", String(req.auto_name));
    if (req.provisioning) form.append("provisioning", JSON.stringify(req.provisioning));
    if (req.extensions && Object.keys(req.extensions).length > 0)
      form.append("extensions", JSON.stringify(req.extensions));
    for (const file of req.images ?? []) {
      form.append("images", file, file.name);
    }
    // #779: everything that is not an image rides in `files` — same `_input/`
    // destination on the daemon, listed under `## Input Files` for the entry node.
    for (const file of req.files ?? []) {
      form.append("files", file, file.name);
    }

    // FormData → no manual Content-Type, so the browser sets the boundary.
    return request<CreateRunResponse>("POST", "/runs", { body: form, label: "POST /runs" });
  }

  // eslint-disable-next-line @typescript-eslint/no-unused-vars
  const { images: _omitted, files: _omittedFiles, ...jsonBody } = req;
  return request<CreateRunResponse>("POST", "/runs", { body: jsonBody, label: "POST /runs" });
}

/** The body of a mid-run repo-list edit (#465 slice 2). `add` entries mirror the
 *  create-modal secondary rows; `remove` names aliases. Both optional — an empty
 *  body is a legal no-op. */
export interface EditRunReposBody {
  add?: TargetRepoInput[];
  remove?: string[];
}

/**
 * What one `PATCH /runs/{id}/repos` did (#465 slice 2, ADR-0042).
 *
 * A refusal is a **verdict, not an exception** — same contract as {@link markNodeDone}:
 * the daemon names the cause with a slug and prose, so the panel shows it inline
 * rather than throwing a hard error. Only a breakdown of the call itself (network)
 * throws.
 */
export type EditRunReposOutcome =
  | { kind: "ok"; run: RunState }
  | { kind: "refused"; slug: string | null; message: string; status: number };

/**
 * Add / remove read-only secondary repos on a live Run (#465 slice 2, ADR-0042).
 *
 * Returns the reprojected {@link RunState} on success (so the caller refreshes in one
 * round-trip), or a typed refusal on any 4xx/5xx the daemon argues
 * (`run_not_editable`, `secondary_is_primary`, `bad_secondary_repo`, …). Precedent:
 * `PATCH /triggers/{id}` for the verb, {@link markNodeDone} for the verdict shape.
 */
export async function editRunRepos(
  runId: string,
  body: EditRunReposBody,
): Promise<EditRunReposOutcome> {
  // Raw mode: a 4xx/5xx here is a nameable refusal, not a failed call, so we read the
  // body ourselves instead of letting `request` throw.
  const resp = await request<Response>(
    "PATCH",
    `/runs/${encodeURIComponent(runId)}/repos`,
    { body, responseMode: "raw", label: `PATCH /runs/${runId}/repos` },
  );
  const parsed: unknown = await resp.json().catch(() => null);
  if (resp.ok) {
    return { kind: "ok", run: parsed as RunState };
  }
  const b = parsed as { error?: unknown } | null;
  return {
    kind: "refused",
    slug: typeof b?.error === "string" ? b.error : null,
    message: apiErrorMessage(parsed, `edit repos refused: ${resp.status}`),
    status: resp.status,
  };
}
