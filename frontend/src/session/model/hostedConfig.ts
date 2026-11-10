// The configuration of an agent Session hébergée as the agent control of its
// detail reads and writes it (CORE-125).

import type { AgentChoice } from "../../shared/generated/AgentChoice";
import type { AgentProfile } from "../../shared/generated/AgentProfile";
import { resolveAgentChoice } from "../../settings/model/agentProfiles";
import type { AgentCombination } from "../../settings/model/settings";
import type { HostedConfig } from "./hostedSession";

/** The agent of a configuration, as the agent control shows it: the
 *  instance's defaults (Inherit) when it names none, a custom combination
 *  otherwise. */
export function choiceOf(config: HostedConfig, inherited: AgentCombination): AgentChoice {
  if (!config.harness && !config.model && !config.effort) return { mode: "inherit" };
  return {
    mode: "custom",
    harness: config.harness || inherited.harness,
    model: config.model ?? null,
    effort: config.effort ?? null,
  };
}

/** The configuration `choice` makes, its initial prompt kept. A profile is
 *  stored as its combination: the session keeps no profile. */
export function configOf(
  choice: AgentChoice,
  prompt: string | null | undefined,
  profiles: AgentProfile[],
  inherited: AgentCombination,
): HostedConfig {
  const config: HostedConfig = {};
  if (choice.mode !== "inherit") {
    const { combination } = resolveAgentChoice(choice, profiles, inherited);
    config.harness = combination.harness;
    if (combination.model) config.model = combination.model;
    if (combination.effort) config.effort = combination.effort;
  }
  if (prompt) config.prompt = prompt;
  return config;
}
