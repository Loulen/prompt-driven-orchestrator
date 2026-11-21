// The choices the « Open support agent » window offers, and how an open agent
// reads: pure, so the window and the palette share one rule.

import type { AgentChoice } from "../../shared/generated/AgentChoice";
import type { AgentProfile } from "../../shared/generated/AgentProfile";
import type { SupportAgentView } from "../../shared/generated/SupportAgentView";
import type { SupportStatusResponse } from "../../shared/generated/SupportStatusResponse";
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

/** An open agent, in short: `claude · opus`; with no model set, the harness
 *  runs its own default one: `claude · default model`. */
export function agentTag(agent: Pick<SupportAgentView, "harness" | "model">): string {
  return `${agent.harness} · ${agent.model || "default model"}`;
}

/** Where this browser keeps the last answer it saw of the open agent. */
const SEEN_ANSWER_KEY = "pdo.supportAgent.seenAnswer";

/** What the last answer seen belongs to: one opening of one agent; `null`
 *  when no agent lives. */
export function answerSeenKey(status: Pick<SupportStatusResponse, "session" | "alive" | "agent">): string | null {
  return status.alive && status.agent ? `${status.session}@${status.agent.opened_at}` : null;
}

/** The last answer this browser saw of the agent `key` names, when its
 *  terminal was minimized; `null` when it never was (another opening, a
 *  storage that cannot be read). */
export function readSeenAnswer(key: string | null): string | null {
  if (!key) return null;
  try {
    const stored = JSON.parse(localStorage.getItem(SEEN_ANSWER_KEY) ?? "null") as { key?: string; seen?: string } | null;
    return stored?.key === key && typeof stored.seen === "string" ? stored.seen : null;
  } catch {
    return null;
  }
}

/** Keep `seen` as the last answer this browser saw of the agent `key` names:
 *  a reload, or another tab, reads it back. */
export function writeSeenAnswer(key: string | null, seen: string): void {
  if (!key) return;
  try {
    localStorage.setItem(SEEN_ANSWER_KEY, JSON.stringify({ key, seen }));
  } catch {
    // No storage: the dot lives as long as the page.
  }
}

/** Whether the live agent wrote after `seen`, the answer seen when its
 *  terminal was last minimized. Never minimized: no dot. */
export function hasNewAnswer(
  status: Pick<SupportStatusResponse, "alive" | "last_output_at"> | null,
  seen: string | null,
): boolean {
  if (!status?.alive || seen === null || !status.last_output_at) return false;
  return status.last_output_at > seen;
}

/** A delay without input, said short: `30 min`, `90 s`. */
export function idleDelay(secs: number): string {
  return secs >= 60 && secs % 60 === 0 ? `${secs / 60} min` : `${secs} s`;
}

/** The countdown of « Still there? », in seconds under its minute: `0:60`,
 *  `0:09`, never below `0:00`. */
export function countdown(ms: number): string {
  const secs = Math.max(0, Math.ceil(ms / 1000));
  if (secs <= 60) return `0:${String(secs).padStart(2, "0")}`;
  return `${Math.floor(secs / 60)}:${String(secs % 60).padStart(2, "0")}`;
}
