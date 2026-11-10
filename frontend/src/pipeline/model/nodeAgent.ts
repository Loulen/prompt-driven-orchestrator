import type { AgentChoice } from "../../shared/generated/AgentChoice";
import type { NodeType } from "../../shared/generated/NodeType";

interface ProfileBrief {
  id: string;
  name: string;
  harness: string;
  model?: string;
  effort?: string;
}

/** What runs a node, as its canvas card names it: a short word and the full line. */
export interface NodeAgentLabel {
  short: string;
  title: string;
}

const line = (...parts: (string | null | undefined)[]) => parts.filter(Boolean).join(" · ");

/**
 * The agent of a node: `bash` for a script, the model of a custom agent or of
 * its profile, the harness a Run froze it on, else « inherit ». `nameOf` names
 * a profile the list no longer has (by default, its id).
 */
export function nodeAgentLabel(
  node: { type: NodeType; agent_choice?: AgentChoice | null },
  profiles: readonly ProfileBrief[],
  frozenHarness?: string | null,
  nameOf: (profileId: string) => string = (profileId) => profileId,
): NodeAgentLabel {
  if (node.type === "script") return { short: "bash", title: "Script · bash" };
  const choice = node.agent_choice;
  if (choice?.mode === "custom") {
    return { short: choice.model || choice.harness, title: line(choice.harness, choice.model, choice.effort) };
  }
  if (choice?.mode === "profile") {
    const profile = profiles.find((p) => p.id === choice.profile_id);
    if (!profile) return { short: "profile", title: `Missing profile ${nameOf(choice.profile_id)}` };
    return {
      short: profile.model?.split("/").pop() || profile.name,
      title: line(profile.name, profile.harness, profile.model, profile.effort),
    };
  }
  if (frozenHarness) return { short: frozenHarness, title: frozenHarness };
  return { short: "inherit", title: "Inherits the run agent" };
}
