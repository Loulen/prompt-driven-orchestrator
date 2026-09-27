import type { PipelineListEntry } from "../../shared/generated/PipelineListEntry";
import { presetToCron, type CronPresetId } from "../../cronPresets";

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
 * The pipelines a filter fragment leaves standing in the Pipeline menu (#822).
 *
 * Case-insensitive substring on the NAME only. The name is what the menu shows, so
 * matching it is the only rule whose result a reader can check by looking: matching
 * the id (a file stem) or the path would keep rows whose visible text contains
 * nothing of what was typed. A blank fragment matches everything — the menu opens on
 * the full list and costs nothing to whoever types nothing.
 *
 * Order is preserved (the daemon's, i.e. the one the old `<select>` had), so the
 * "first pipeline" default and the keyboard cursor never jump as characters land.
 */
export function filterPipelines(
  pipelines: PipelineListEntry[],
  query: string,
): PipelineListEntry[] {
  const needle = query.trim().toLowerCase();
  if (!needle) return pipelines;
  return pipelines.filter((p) => p.name.toLowerCase().includes(needle));
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

// The sandbox / harness clusters and the request bodies live in sibling files; re-exported
// here so existing importers keep one entry point.
export { sandboxState, harnessState } from "./newRunFormSelectors";
export type { SandboxState, HarnessState } from "./newRunFormSelectors";
export {
  buildRunPayload,
  isImageAttachment,
  buildTriggerUpdatePayload,
  buildTriggerCreatePayload,
} from "./newRunFormPayload";
export type { RunPayloadInput, TriggerPayloadInput } from "./newRunFormPayload";
