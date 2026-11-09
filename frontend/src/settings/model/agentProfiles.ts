import type { AgentChoice } from "../../shared/generated/AgentChoice";
import type { AgentCombination } from "./settings";
import type { AgentProfile } from "../../shared/generated/AgentProfile";

/** The reserved id of the instance's floor profile (ADR-0057 ¶3). */
export const DEFAULT_PROFILE_ID = "default";

/** What « Inherit » falls to when every tier above is transparent, as the
 *  daemon resolves it: the instance's own choice when it names a profile that
 *  exists or a custom combination, else its legacy default harness (without
 *  model or effort), else the Default profile, edited or not, else `claude`
 *  until the profiles load. */
export function inheritedFloor(
  profiles: AgentProfile[],
  instanceDefaultHarness?: string | null,
  instanceChoice?: AgentChoice | null,
): AgentCombination {
  if (instanceChoice?.mode === "custom" && instanceChoice.harness.trim()) {
    return { harness: instanceChoice.harness, model: instanceChoice.model ?? null, effort: instanceChoice.effort ?? null };
  }
  if (instanceChoice?.mode === "profile") {
    const named = profiles.find((candidate) => candidate.id === instanceChoice.profile_id);
    if (named) return { harness: named.harness, model: named.model ?? null, effort: named.effort ?? null };
  }
  if (instanceDefaultHarness) return { harness: instanceDefaultHarness, model: null, effort: null };
  const floor = profiles.find((candidate) => candidate.id === DEFAULT_PROFILE_ID);
  return floor
    ? { harness: floor.harness, model: floor.model ?? null, effort: floor.effort ?? null }
    : { harness: "claude", model: null, effort: null };
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
