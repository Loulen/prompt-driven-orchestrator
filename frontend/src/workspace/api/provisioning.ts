/**
 * Provisioning rules over HTTP: the instance's, a Projet's, and the preview the
 * editors resolve against the source repository.
 */

import { request } from "../../shared/http/client";
import type { ProvisioningPlan } from "../../shared/generated/ProvisioningPlan";
import type { ProvisioningRules } from "../../shared/generated/ProvisioningRules";
import type { ProvisioningScope } from "../../shared/generated/ProvisioningScope";
import type { ScopedProvisioningRules } from "../../shared/generated/ScopedProvisioningRules";

export function previewProvisioning(
  repository: string,
  scope: ProvisioningScope,
  rules: ProvisioningRules,
  inherited?: ScopedProvisioningRules[],
  gitRef = "HEAD",
): Promise<ProvisioningPlan> {
  return request("POST", "/repos/provisioning/preview", {
    body: { repository, git_ref: gitRef, scope, rules, inherited },
  });
}

export function fetchInstanceProvisioning(): Promise<ProvisioningRules> {
  return request("GET", "/settings/provisioning");
}

export function saveInstanceProvisioning(
  rules: ProvisioningRules,
): Promise<ProvisioningRules> {
  return request("PUT", "/settings/provisioning", { body: rules });
}

export function fetchProjectProvisioning(projectId: string): Promise<ProvisioningRules> {
  return request("GET", `/projects/${encodeURIComponent(projectId)}/provisioning`);
}

export function saveProjectProvisioning(
  projectId: string,
  rules: ProvisioningRules,
): Promise<ProvisioningRules> {
  return request("PUT", `/projects/${encodeURIComponent(projectId)}/provisioning`, {
    body: rules,
  });
}
