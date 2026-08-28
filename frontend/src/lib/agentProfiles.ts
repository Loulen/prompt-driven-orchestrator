import type { AgentChoice, AgentCombination, AgentProfile } from "../types";

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
