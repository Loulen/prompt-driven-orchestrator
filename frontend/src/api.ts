import type { InstanceSettings, AgentProfileReferents } from "./settings/model/settings";
import type { UpdateSettingsRequest } from "./shared/generated/UpdateSettingsRequest";
import type { AgentProfile } from "./shared/generated/AgentProfile";
import type { AgentProfileInput } from "./shared/generated/AgentProfileInput";
import { request } from "./shared/http/client";

export function fetchSettings(): Promise<InstanceSettings> {
  return request<InstanceSettings>("GET", "/settings");
}

/**
 * Persist one or more instance-config knobs and return the recomputed view
 * (#129, ADR-0015). Surfaces the daemon's fail-fast validation error (`400`)
 * verbatim so the modal can show it.
 */
export function updateSettings(
  patch: UpdateSettingsRequest,
): Promise<InstanceSettings> {
  return request<InstanceSettings>("PUT", "/settings", { body: patch });
}

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

export function fetchAgentProfileReferents(id: string): Promise<AgentProfileReferents> {
  return request(
    "GET",
    `/settings/agent-profiles/${encodeURIComponent(id)}/referents`,
  );
}
