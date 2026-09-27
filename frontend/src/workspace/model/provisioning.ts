import type { ProvisioningRules } from "../../shared/generated/ProvisioningRules";
import type { ProvisioningScope } from "../../shared/generated/ProvisioningScope";

export const EMPTY_PROVISIONING_RULES: ProvisioningRules = {
  copy: [],
  hardlink: [],
  symlink: [],
};

export function hasProvisioningRules(rules: ProvisioningRules): boolean {
  return rules.copy.length > 0 || rules.hardlink.length > 0 || rules.symlink.length > 0;
}

/** The provisioning levels, coarsest first. */
export const SCOPES: ProvisioningScope[] = [
  "instance",
  "project",
  "run",
  "isolated_node",
];

/** Whether `left` is a coarser level than `right`. */
export function scopePrecedes(left: ProvisioningScope, right: ProvisioningScope): boolean {
  return SCOPES.indexOf(left) < SCOPES.indexOf(right);
}

/** Whether two path lists share a path, or one holds an ancestor of the other. */
export function pathsOverlap(left: string[], right: string[]): boolean {
  return left.some((leftPath) =>
    right.some(
      (rightPath) =>
        leftPath === rightPath ||
        leftPath.startsWith(`${rightPath}/`) ||
        rightPath.startsWith(`${leftPath}/`),
    ),
  );
}

/** The non-blank, trimmed lines of a rule textarea: one pattern per line. */
export function ruleLines(value: string): string[] {
  return value
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean);
}
