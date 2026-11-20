// The choices the « Open support agent » window offers, and how an open agent
// reads: pure, so the window and the palette share one rule.

import type { AgentChoice } from "../../shared/generated/AgentChoice";
import type { AgentProfile } from "../../shared/generated/AgentProfile";
import type { SupportAgentView } from "../../shared/generated/SupportAgentView";
import type { HarnessCatalog } from "../../harness/model/catalog";
import type { AgentCombination } from "../../settings/model/settings";
import { DEFAULT_PROFILE_ID } from "../../settings/model/agentProfiles";

/** The catalogue without the harnesses whose binary is not installed here. */
export function installedCatalog(catalog: HarnessCatalog): HarnessCatalog {
  return {
    builtin: catalog.builtin.filter((h) => h.installed),
    descriptors: catalog.descriptors.filter((h) => h.installed),
  };
}

const installedNames = (catalog: HarnessCatalog) =>
  new Set([...catalog.builtin, ...catalog.descriptors].filter((h) => h.installed).map((h) => h.name));

/** The profiles whose harness is installed here: the others would only fail. */
export function runnableProfiles(profiles: AgentProfile[], catalog: HarnessCatalog): AgentProfile[] {
  const installed = installedNames(catalog);
  return profiles.filter((p) => installed.has(p.harness));
}

/** Whether `choice` can be opened here: a runnable profile, or a custom
 *  combination on an installed harness. */
export function isRunnable(choice: AgentChoice | null | undefined, profiles: AgentProfile[], catalog: HarnessCatalog): boolean {
  if (!choice) return false;
  if (choice.mode === "profile") return runnableProfiles(profiles, catalog).some((p) => p.id === choice.profile_id);
  if (choice.mode === "custom") return installedNames(catalog).has(choice.harness);
  return false;
}

/** The choice the window opens on: the last one when it still runs here, else
 *  the Default profile, else the first runnable profile, else the first
 *  installed harness; `null` when nothing is installed. */
export function initialChoice(
  last: AgentChoice | null | undefined,
  profiles: AgentProfile[],
  catalog: HarnessCatalog,
): AgentChoice | null {
  if (last && isRunnable(last, profiles, catalog)) return last;
  const runnable = runnableProfiles(profiles, catalog);
  const profile = runnable.find((p) => p.id === DEFAULT_PROFILE_ID) ?? runnable[0];
  if (profile) return { mode: "profile", profile_id: profile.id };
  const harness = [...catalog.builtin, ...catalog.descriptors].find((h) => h.installed);
  return harness ? { mode: "custom", harness: harness.name, model: null, effort: null } : null;
}

/** The combination `choice` runs, for the custom pane to open on. */
export function choiceCombination(choice: AgentChoice | null, profiles: AgentProfile[]): AgentCombination {
  if (choice?.mode === "custom") return { harness: choice.harness, model: choice.model ?? null, effort: choice.effort ?? null };
  const profile = choice?.mode === "profile" ? profiles.find((p) => p.id === choice.profile_id) : undefined;
  return { harness: profile?.harness ?? "claude", model: profile?.model ?? null, effort: profile?.effort ?? null };
}

/** What `choice` is called: its profile's name, or « Custom ». */
export function choiceLabel(choice: AgentChoice | null, profiles: AgentProfile[]): string {
  if (choice?.mode === "profile") return profiles.find((p) => p.id === choice.profile_id)?.name ?? "this agent";
  if (choice?.mode === "custom") return choice.harness;
  return "this agent";
}

/** An open agent, in short: `claude · opus`, the model left out when unset. */
export function agentTag(agent: Pick<SupportAgentView, "harness" | "model">): string {
  return [agent.harness, agent.model].filter(Boolean).join(" · ");
}
