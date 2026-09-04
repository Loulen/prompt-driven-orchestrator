import type {
  CreateRunRequest,
  CreateTriggerRequest,
  TargetRepoInput,
  UpdateTriggerRequest,
} from "../api";
import type { InstanceSettings, PipelineListEntry, SandboxProfileRef, SkillRef } from "../types";
import { presetToCron, type CronPresetId } from "../cronPresets";
import { HARNESS_FLOOR, harnessCatalog, type HarnessCatalog } from "./harness";

/**
 * The New Run / Trigger form's pure logic (#359), lifted out of `NewRunModal.tsx`.
 *
 * Everything here is a function of the form's values — no state, no fetch, no React — so
 * the decisions the dialog makes can be read and tested without rendering it: what a
 * variable override parses to, what the sandbox selector actually resolves to, whether
 * Launch / Create is allowed, and the exact request bodies that get posted.
 *
 * The modal keeps the JSX shell, the `mode` toggle and the wiring.
 */

/** What the override inputs hold: the raw typed text, keyed by variable name. */
type VariableOverrides = Record<string, string>;

export function parseVariableValue(raw: string, varType: string): unknown {
  switch (varType) {
    case "int":
      return parseInt(raw, 10) || 0;
    case "float":
      return parseFloat(raw) || 0;
    case "bool":
      return raw === "true";
    case "list":
      try {
        return JSON.parse(raw);
      } catch {
        return raw
          .replace(/^\[|\]$/g, "")
          .split(",")
          .map((s) => s.trim());
      }
    default:
      return raw;
  }
}

/**
 * The variables a submit sends: only the overrides the selected pipeline actually
 * DECLARES, and only those that differ from the declared default — an override equal to
 * the default is not an override, so it stays out of the payload rather than pinning a
 * value the pipeline may later change.
 *
 * Run mode and Trigger mode had a byte-identical copy of this loop each (#359); they now
 * share this one, so the two can no longer drift apart.
 */
export function buildVariables(
  overrides: VariableOverrides,
  pipeline: PipelineListEntry,
): Record<string, unknown> {
  const variables: Record<string, unknown> = {};
  for (const [key, val] of Object.entries(overrides)) {
    const decl = pipeline.variables[key];
    if (!decl) continue;
    if (val === String(decl.default)) continue;
    variables[key] = parseVariableValue(val, decl.var_type);
  }
  return variables;
}

/** How many of the typed overrides are real overrides — the accordion's badge. */
export function overrideCount(
  overrides: VariableOverrides,
  pipeline: PipelineListEntry | undefined,
): number {
  if (!pipeline) return 0;
  return Object.entries(overrides).filter(([key, val]) => {
    const decl = pipeline.variables[key];
    if (!decl) return false;
    return val !== String(decl.default);
  }).length;
}

/**
 * The cron expression the Trigger will be created with: a compiled preset or the raw
 * escape-hatch expression.
 */
export function resolvedCron({
  cronPresetId,
  rawCron,
  dailyHour,
  dailyMinute,
}: {
  cronPresetId: CronPresetId;
  rawCron: string;
  dailyHour: number;
  dailyMinute: number;
}): string {
  return cronPresetId === "custom"
    ? rawCron.trim()
    : presetToCron(cronPresetId, { hour: dailyHour, minute: dailyMinute });
}

/**
 * A prompt-optional pipeline (#158) may launch with an empty prompt; the entry node
 * sources its own work. Prompt-required (the default) still demands non-empty input.
 */
export function promptOptional(pipeline: PipelineListEntry | undefined): boolean {
  return pipeline?.prompt_required === false;
}

/** @see promptOptional */
export function hasRequiredPrompt(promptOptional: boolean, input: string): boolean {
  return promptOptional || Boolean(input.trim());
}

/**
 * The fire_decision reject rule, mirrored client-side: a prompt-required pipeline whose
 * resolved input would be empty (no guard, no input template) is a misconfiguration. We
 * pre-block Create and explain why, in addition to the authoritative server-side reject
 * (CONTEXT.md → Trigger; #161).
 */
export function triggerInputRejectReason({
  mode,
  selectedPipeline,
  promptOptional,
  guardCommand,
  input,
}: {
  mode: "run" | "trigger";
  selectedPipeline: PipelineListEntry | undefined;
  promptOptional: boolean;
  guardCommand: string;
  input: string;
}): string | null {
  return mode === "trigger" &&
    selectedPipeline &&
    !promptOptional &&
    guardCommand.trim().length === 0 &&
    input.trim().length === 0
    ? "This pipeline requires a prompt. Add a guard command, an input template, or mark the pipeline prompt-not-required."
    : null;
}

export function canLaunch({
  repoValid,
  selectedPipeline,
  hasRequiredPrompt,
  missingProfile,
  sandboxDoomed,
  // #465: every non-empty secondary repo row must have resolved to a valid repo
  // (mirror of the primary `repoValid` gate). Defaults to `true` — a mono-repo Run
  // has no secondaries to gate on.
  secondariesReady = true,
}: {
  repoValid: boolean | null;
  selectedPipeline: PipelineListEntry | undefined;
  hasRequiredPrompt: boolean;
  missingProfile: boolean;
  sandboxDoomed: boolean;
  secondariesReady?: boolean;
}): boolean {
  return Boolean(
    repoValid &&
      selectedPipeline &&
      hasRequiredPrompt &&
      !missingProfile &&
      !sandboxDoomed &&
      secondariesReady,
  );
}

/**
 * Trigger creation needs a name, a pipeline, a valid repo and a cron, and a resolvable
 * input when the pipeline requires a prompt.
 */
export function canCreateTrigger({
  repoValid,
  selectedPipeline,
  triggerName,
  resolvedCron,
  triggerInputRejectReason,
  missingProfile,
}: {
  repoValid: boolean | null;
  selectedPipeline: PipelineListEntry | undefined;
  triggerName: string;
  resolvedCron: string;
  triggerInputRejectReason: string | null;
  missingProfile: boolean;
}): boolean {
  return Boolean(
    repoValid &&
      selectedPipeline &&
      triggerName.trim().length > 0 &&
      resolvedCron.length > 0 &&
      !triggerInputRejectReason &&
      // #432: a Trigger pointing at a vanished profile must not be re-saved as-is; the
      // user picks a real one (or `off`) first.
      !missingProfile,
  );
}

/** Everything the sandbox selector derives from `settings` + the selected value (ADR-0031). */
export interface SandboxState {
  dockerUnavailable: boolean;
  sandboxReason: string | undefined;
  sandboxProfiles: SandboxProfileRef[];
  missingProfile: boolean;
  instanceDefaultSandbox: string | null;
  effectiveSandbox: string | null;
  sandboxDoomed: boolean;
  inheritedDefaultReason: string | null;
}

/**
 * The sandbox cluster (#410/#432/#452, ADR-0031 §7). `sandbox` is the selector value in
 * BOTH modes: `""` = "the user did not choose", `off`, or a staging profile name.
 */
export function sandboxState({
  settings,
  sandbox,
  mode,
}: {
  settings: InstanceSettings | null;
  sandbox: string;
  mode: "run" | "trigger";
}): SandboxState {
  // #410: advisory Docker greying. Only gate the sandboxed options once we KNOW Docker is
  // unavailable (settings loaded && probe false); while settings load, stay
  // optimistic. `sandboxReason` explains the greying (title + help text).
  const dockerUnavailable = settings != null && !settings.sandbox_docker.available;
  const sandboxReason = settings?.sandbox_docker.reason ?? undefined;

  // #432: the options come from the daemon's profile list. Sorted server-side.
  const sandboxProfiles = settings?.sandbox_profiles ?? [];

  /**
   * THE PHANTOM-PROFILE RULE. A seeded value is **never** silently rewritten: a
   * non-empty, non-`off` value that is absent from the list gets a tombstone option and
   * blocks Save/Launch.
   *
   * Without the tombstone React sets `selectedIndex = -1`, the field renders blank, and
   * saving would PATCH `sandbox: null` — a **silent fallback to the instance default**,
   * exactly what ADR-0031 §7 forbids. Deliberately separate from the Docker clamp above:
   * clamping to `off` is legitimate for an unavailable Docker, and would be a silent
   * fallback for a missing profile.
   */
  const missingProfile = Boolean(
    settings &&
      sandbox &&
      sandbox !== "off" &&
      !sandboxProfiles.some((p) => p.name === sandbox),
  );

  // #452: the instance default, used to LABEL the inherit option — never to seed the value.
  // `null` while settings are unknown, which is the honest rendering: we do not know yet.
  const instanceDefaultSandbox = settings ? (settings.default_sandbox.effective ?? "off") : null;

  // #452: what will actually apply to the Run being created. `""` is not a value — it means
  // the key is omitted and the instance default decides — so the checks below have to
  // resolve it to say anything true about the Run.
  const effectiveSandbox = sandbox === "" ? instanceDefaultSandbox : sandbox;

  /**
   * #452, the Docker clamp, relocated. A Run whose effective sandbox is a profile while the
   * daemon reports Docker unavailable is born condemned, so #410 clamped the SELECTOR to
   * `off`. That protection was right and is kept; the way it was applied was not — it wrote
   * a business verdict into the field, indistinguishable from a user picking `off`, and
   * posted it explicitly.
   *
   * So refuse the launch and say why, instead of quietly substituting an answer. Same rule
   * as the phantom-profile tombstone above, for the same reason: the app never demotes a
   * sandbox behind the user's back (ADR-0031 §7).
   *
   * Run mode only. A Trigger resolves its sandbox when it FIRES, and today's Docker probe
   * says nothing about that moment.
   */
  const sandboxDoomed = Boolean(
    mode === "run" && dockerUnavailable && effectiveSandbox && effectiveSandbox !== "off",
  );

  // #452: `default_sandbox` carries a `reason` when the winning tier names a profile that
  // does not resolve. Inheriting is now the default path in run mode, so that dangling name
  // is worth showing here — the create chokepoint 400s on it rather than falling back.
  const inheritedDefaultReason = sandbox === "" ? (settings?.default_sandbox.reason ?? null) : null;

  return {
    dockerUnavailable,
    sandboxReason,
    sandboxProfiles,
    missingProfile,
    instanceDefaultSandbox,
    effectiveSandbox,
    sandboxDoomed,
    inheritedDefaultReason,
  };
}

/** Everything the harness selector derives from `settings` + the selected value (#551, ADR-0046). */
export interface HarnessState {
  /** The harnesses the selector offers, split into Built-in / From-descriptors
   *  sections with per-harness install state (#586, dynamic from `/settings`). */
  catalog: HarnessCatalog;
  /**
   * The instance default harness, used to **label** the inherit option — never to seed
   * the value (the #452 trap: a prefilled field freezes a stale value the user never
   * chose). `null` while `settings` are unknown (the honest "we don't know yet" render).
   * Resolves through the floor when the instance names none, so run mode always has a
   * concrete name to show.
   */
  instanceDefaultHarness: string | null;
  /**
   * What will actually apply to the Run being created. `""` is not a value — it means the
   * key is omitted and the instance default (then the floor) decides — so it resolves to
   * `instanceDefaultHarness`. A concrete selection is itself.
   */
  effectiveHarness: string | null;
}

/**
 * The harness cluster (#551, ADR-0046). `harness` is the selector value in BOTH modes:
 * `""` = "the user did not choose" (inherit), or a concrete harness name. Mirror of
 * {@link sandboxState}, minus the Docker/profile machinery — a harness name is free text
 * with no availability probe (ADR-0045: PDO does not validate it; an unknown one fails
 * fast at spawn), so there is nothing to grey and nothing to block.
 */
export function harnessState({
  settings,
  harness,
}: {
  settings: InstanceSettings | null;
  harness: string;
}): HarnessState {
  // #452: the instance default LABELS the inherit option, never seeds the value. `null`
  // while settings are unknown; resolves through the `claude` floor once known, so run
  // mode always names a concrete harness the Run would inherit.
  const instanceDefaultHarness = settings
    ? (settings.default_harness.effective || HARNESS_FLOOR)
    : null;
  const effectiveHarness = harness === "" ? instanceDefaultHarness : harness;
  return {
    // #586: the picker's options are now dynamic — the floor merged with the disk
    // descriptor tier, each tagged installed/not. `harnessCatalog` falls back to
    // the embedded floor while settings are unknown, so the control is never empty.
    catalog: harnessCatalog(settings?.harness_descriptors ?? null),
    instanceDefaultHarness,
    effectiveHarness,
  };
}

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
  images: File[];
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
  images,
  targetRepos,
}: RunPayloadInput): CreateRunRequest {
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
  };
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
