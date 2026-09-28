import type { CreateTriggerRequest, UpdateTriggerRequest } from "../../trigger/api/triggers";
import type { CreateRunRequest, TargetRepoInput } from "../api/createRun";
import type { PipelineListEntry } from "../../shared/generated/PipelineListEntry";
import type { SkillRef } from "../../shared/generated/SkillRef";

/**
 * The New Run / Trigger form's request bodies, split out of `newRunForm.ts`: the exact
 * payloads `POST /runs`, `POST /triggers` and `PATCH /triggers/:id` receive.
 */

/** The form values `POST /runs` reads. */
export interface RunPayloadInput {
  selectedPipeline: PipelineListEntry;
  input: string;
  variables: Record<string, unknown>;
  targetRepo: string;
  sourceBranch: string;
  autoName: boolean;
  runName: string;
  sandbox: string;
  /** #551: the harness selector value — `""` (inherit) or a concrete name. */
  harness: string;
  /** #669: the Run tier of the skills selection. Empty ⇒ the key is omitted. */
  skills?: SkillRef[];
  /** #779: every attachment of the modal, images and files alike. Split by MIME at
   *  build time: `image/*` → `images`, the rest → `files`. */
  attachments: File[];
  /** #465: `[0]` = primary, `[1..]` = secondaries. Omit / `undefined` for a mono-repo Run. */
  targetRepos?: TargetRepoInput[];
}

export function buildRunPayload({
  selectedPipeline,
  input,
  variables,
  targetRepo,
  sourceBranch,
  autoName,
  runName,
  sandbox,
  harness,
  skills,
  attachments,
  targetRepos,
}: RunPayloadInput): CreateRunRequest {
  const images = attachments.filter(isImageAttachment);
  const files = attachments.filter((f) => !isImageAttachment(f));
  return {
    pipeline: selectedPipeline.name,
    input: input.trim(),
    variables,
    pipeline_id: selectedPipeline.id,
    target_repo: targetRepo.trim() || undefined,
    // #465: send the full list only when there is ≥1 secondary — a mono-repo Run
    // keeps the request byte-identical (the key is omitted).
    target_repos: targetRepos,
    source_branch: sourceBranch || undefined,
    name: autoName ? undefined : runName.trim() || undefined,
    // #338: always send the explicit choice so it wins the create-chokepoint
    // resolution (the daemon never has to guess from `name` presence for a UI create).
    auto_name: autoName,
    // #410/#452: the explicit run-level choice — `off` or a staging profile name — sent
    // so it wins the create-chokepoint precedence. `""` means the user did not choose,
    // and OMITS the key: only an absent `sandbox` lets the daemon apply
    // `default_sandbox`, because it reads a present `off` as final.
    sandbox: sandbox || undefined,
    // #551: the explicit harness choice. `""` (inherit) OMITS the key, so the Run names
    // no harness and each free node resolves through the instance default and the floor —
    // exactly the "name the default, don't copy it" contract of the selector (#452).
    harness: harness || undefined,
    // #669: the explicit Run-tier skills. An empty list OMITS the key, so the
    // Run adds none and the payload stays byte-identical to the pre-#669 shape.
    skills: skills && skills.length > 0 ? skills : undefined,
    images: images.length > 0 ? images : undefined,
    files: files.length > 0 ? files : undefined,
  };
}

/** #779: an attachment is an image when its MIME type says so (`image/*`). */
export function isImageAttachment(file: File): boolean {
  return file.type.startsWith("image/");
}

/** The form values a Trigger create / edit reads. */
export interface TriggerPayloadInput {
  selectedPipeline: PipelineListEntry;
  triggerName: string;
  resolvedCron: string;
  input: string;
  guardCommand: string;
  targetRepo: string;
  sourceBranch: string;
  allowOverlap: boolean;
  maxConcurrent: string;
  sandbox: string;
  /** #551: the harness selector value — `""` (inherit) or a concrete name. */
  harness: string;
  /** #669: the Run-tier skills every fired Run carries. */
  skills?: SkillRef[];
  autoName: boolean;
  variables: Record<string, unknown>;
  /** #465: `[0]` = primary, `[1..]` = secondaries. Omit / `undefined` for a mono-repo Trigger. */
  targetRepos?: TargetRepoInput[];
}

/**
 * Edit (#162): PATCH the existing Trigger's editable fields. `Some(None)`
 * semantics: an emptied guard clears it. `pipeline_id` repoints the
 * trigger to a different pipeline (#230) — previously the editable
 * dropdown was a phantom control whose change was silently dropped.
 */
export function buildTriggerUpdatePayload({
  selectedPipeline,
  triggerName,
  resolvedCron,
  input,
  guardCommand,
  targetRepo,
  sourceBranch,
  allowOverlap,
  maxConcurrent,
  sandbox,
  harness,
  skills,
  autoName,
  variables,
  targetRepos,
}: TriggerPayloadInput): UpdateTriggerRequest {
  return {
    name: triggerName.trim(),
    pipeline_id: selectedPipeline.id,
    cron: resolvedCron,
    input_template: input.trim(),
    guard_command: guardCommand.trim() || null,
    target_repo: targetRepo.trim() || null,
    // #465: an array sets the secondaries; `null` clears back to mono-repo.
    target_repos: targetRepos ?? null,
    source_branch: sourceBranch || null,
    // Round-trip the real overlap policy (#239). Previously hard-coded to
    // `undefined`, which silently reset every edited trigger toward skip.
    // `null` clears a stale cap when overlap is off or the input is blank.
    overlap_policy: allowOverlap ? "allow" : "skip",
    max_concurrent: allowOverlap && maxConcurrent.trim() ? Number(maxConcurrent) : null,
    // #410: `""` (Use instance default) clears back to inheriting (`null`);
    // `off` or a staging profile name sets it.
    sandbox: sandbox || null,
    // #551: `""` (Use instance default) clears back to inheriting (`null`); a concrete
    // harness name sets it. Mirror of `sandbox`.
    harness: harness || null,
    // #669: replaced wholesale on edit — an empty list clears the Trigger's skills.
    skills: skills ?? [],
    // #338: round-trip the auto-naming choice (flat bool, mirror of `enabled`).
    auto_name: autoName,
    variables,
  };
}

export function buildTriggerCreatePayload({
  selectedPipeline,
  triggerName,
  resolvedCron,
  input,
  guardCommand,
  targetRepo,
  sourceBranch,
  allowOverlap,
  maxConcurrent,
  sandbox,
  harness,
  skills,
  autoName,
  variables,
  targetRepos,
}: TriggerPayloadInput): CreateTriggerRequest {
  return {
    name: triggerName.trim(),
    pipeline_id: selectedPipeline.id,
    cron: resolvedCron,
    input_template: input.trim() || undefined,
    guard_command: guardCommand.trim() || undefined,
    target_repo: targetRepo.trim() || undefined,
    // #465: send the secondaries only when there is ≥1 (else omit → mono-repo).
    target_repos: targetRepos,
    source_branch: sourceBranch || undefined,
    overlap_policy: allowOverlap ? "allow" : "skip",
    max_concurrent: allowOverlap && maxConcurrent.trim() ? Number(maxConcurrent) : undefined,
    // #410: `""` (Use instance default) → `null` (inherit); `off` or a profile sets it.
    sandbox: sandbox || null,
    // #551: `""` (Use instance default) → `null` (inherit); a concrete harness sets it.
    harness: harness || null,
    // #669: the Run-tier skills every fired Run carries; omitted when none.
    ...(skills && skills.length > 0 ? { skills } : {}),
    // #338: freeze the auto-naming choice on the new Trigger (seeded from the
    // instance default when the modal opened).
    auto_name: autoName,
    variables,
  };
}
