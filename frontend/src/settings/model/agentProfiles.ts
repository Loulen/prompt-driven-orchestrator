import type { AgentChoice } from "../../shared/generated/AgentChoice";
import type { AgentCombination, InstanceSettings } from "./settings";
import type { AgentProfile } from "../../shared/generated/AgentProfile";

/** The reserved id of the instance's floor profile (ADR-0057 ¶3). */
export const DEFAULT_PROFILE_ID = "default";

/** A node's legacy harness fields (pre-#563 YAML): its pin and its
 *  per-harness `{model, effort}` entries. */
export interface NodeHarnessEntries {
  pin_harness?: string | null;
  harnesses?: Record<string, { model?: string | null; effort?: string | null } | undefined> | null;
}

/** The instance's tier, as `GET /settings` serves it: its own choice, its
 *  legacy default harness and its default model per harness. */
export interface InstanceFloor {
  choice?: AgentChoice | null;
  defaultHarness?: string | null;
  defaultModels?: Record<string, string> | null;
}

/** The instance's tier out of its settings (absent until they load). */
export function instanceFloor(
  settings?: Pick<InstanceSettings, "default_harness" | "default_harness_model" | "agent_choice"> | null,
): InstanceFloor {
  return {
    choice: settings?.agent_choice,
    defaultHarness: settings?.default_harness?.effective,
    defaultModels: settings?.default_harness_model?.effective,
  };
}

/** What « Inherit » falls to when every tier above is transparent, as the
 *  daemon resolves it (`agent_choice::resolve`): a node's legacy pin first,
 *  then the instance's own choice when it names a profile that exists or a
 *  custom combination, else its legacy default harness, else the Default
 *  profile, edited or not, else `claude` until the profiles load.
 *
 *  A pin or the default harness runs on the node's own entry for it, its model
 *  else the instance's default model for that harness. At the Default profile,
 *  the node's entry for the profile's harness lays its fields over the
 *  profile's. An instance's choice wins whole. */
export function inheritedFloor(
  profiles: AgentProfile[],
  instance: InstanceFloor = {},
  node?: NodeHarnessEntries,
): AgentCombination {
  const entry = (harness: string) => node?.harnesses?.[harness];
  const legacy = (harness: string): AgentCombination => ({
    harness,
    model: entry(harness)?.model || instance.defaultModels?.[harness] || null,
    effort: entry(harness)?.effort || null,
  });
  if (node?.pin_harness) return legacy(node.pin_harness);
  const choice = instance.choice;
  if (choice?.mode === "custom" && choice.harness.trim()) {
    return { harness: choice.harness, model: choice.model ?? null, effort: choice.effort ?? null };
  }
  if (choice?.mode === "profile") {
    const named = profiles.find((candidate) => candidate.id === choice.profile_id);
    if (named) return { harness: named.harness, model: named.model ?? null, effort: named.effort ?? null };
  }
  if (instance.defaultHarness) return legacy(instance.defaultHarness);
  const floor = profiles.find((candidate) => candidate.id === DEFAULT_PROFILE_ID);
  const harness = floor?.harness ?? "claude";
  const own = entry(harness);
  return { harness, model: own?.model || floor?.model || null, effort: own?.effort || floor?.effort || null };
}

export function combinationLabel(value: AgentCombination): string {
  return [value.harness, value.model || "—", value.effort || "—"].join(" · ");
}

export function resolveAgentChoice(
  choice: AgentChoice | null | undefined,
  profiles: AgentProfile[],
  inherited: AgentCombination,
): { combination: AgentCombination; profile?: AgentProfile; brokenId?: string } {
  if (!choice || choice.mode === "inherit") return { combination: inherited };
  if (choice.mode === "custom") return { combination: choice };
  const profile = profiles.find((candidate) => candidate.id === choice.profile_id);
  return profile
    ? { combination: profile, profile }
    : { combination: inherited, brokenId: choice.profile_id };
}
