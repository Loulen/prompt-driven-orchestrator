import type { AgentProfile } from "../../shared/generated/AgentProfile";
import type { AgentProfileInput } from "../../shared/generated/AgentProfileInput";
import { request } from "../../shared/http/client";
import type { AgentProfileReferents } from "../model/settings";

export function fetchAgentProfiles(): Promise<{ profiles: AgentProfile[] }> {
  return request("GET", "/settings/agent-profiles");
}

export function createAgentProfile(
  profile: AgentProfileInput,
): Promise<AgentProfile> {
  return request("POST", "/settings/agent-profiles", { body: profile });
}

export function updateAgentProfile(
  id: string,
  profile: AgentProfileInput,
): Promise<AgentProfile> {
  return request("PUT", `/settings/agent-profiles/${encodeURIComponent(id)}`, {
    body: profile,
  });
}

export function deleteAgentProfile(id: string): Promise<void> {
  return request("DELETE", `/settings/agent-profiles/${encodeURIComponent(id)}`, {
    responseMode: "void",
  });
}

/** Who names the profile: the instance, the Projets, the Triggers, the pipelines' nodes. */
export function fetchAgentProfileReferents(id: string): Promise<AgentProfileReferents> {
  return request(
    "GET",
    `/settings/agent-profiles/${encodeURIComponent(id)}/referents`,
  );
}
