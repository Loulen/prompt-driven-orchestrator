import type { ProvisioningRules } from "../shared/generated/ProvisioningRules";

export const EMPTY_PROVISIONING_RULES: ProvisioningRules = {
  copy: [],
  hardlink: [],
  symlink: [],
};

export function hasProvisioningRules(rules: ProvisioningRules): boolean {
  return rules.copy.length > 0 || rules.hardlink.length > 0 || rules.symlink.length > 0;
}
