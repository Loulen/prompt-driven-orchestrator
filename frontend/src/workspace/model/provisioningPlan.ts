// What the provisioning editor reads off a resolved plan (or off its own rules without one).
import type { ProvisioningMode } from "../../shared/generated/ProvisioningMode";
import type { ProvisioningPlan } from "../../shared/generated/ProvisioningPlan";
import type { ProvisioningRules } from "../../shared/generated/ProvisioningRules";
import type { ProvisioningScope } from "../../shared/generated/ProvisioningScope";
import type { ScopedProvisioningRules } from "../../shared/generated/ScopedProvisioningRules";
import { SCOPES, scopePrecedes } from "./provisioning";

/** The paths each pattern matched, excluded ones included, per mode. */
export function patternPathCounts(plan: ProvisioningPlan | null): Record<ProvisioningMode, Map<string, number>> {
  const result: Record<ProvisioningMode, Map<string, number>> = {
    copy: new Map(),
    hardlink: new Map(),
    symlink: new Map(),
  };
  for (const rule of plan?.rules ?? []) {
    result[rule.mode].set(rule.pattern, rule.paths.length + rule.excluded_paths.length);
  }
  return result;
}

/** The rules of each level; without a plan, the edited level counts its own rules. */
export function levelRuleCounts(
  plan: ProvisioningPlan | null,
  level: ProvisioningScope,
  rules: ProvisioningRules,
): Map<ProvisioningScope, number> {
  const result = new Map<ProvisioningScope, number>();
  for (const scope of SCOPES) {
    result.set(scope, (plan?.rules ?? []).filter((rule) => rule.scope === scope).length);
  }
  if (!plan) {
    result.set(level, rules.copy.length + rules.hardlink.length + rules.symlink.length);
  }
  return result;
}

/** The header's summary: « none », or the inherited rules and this level's. */
export function provisioningSummary(
  plan: ProvisioningPlan | null,
  failed: boolean,
  level: ProvisioningScope,
  ruleCounts: Map<ProvisioningScope, number>,
  inherited: ScopedProvisioningRules[] | undefined,
): string {
  let inheritedCount = 0;
  if (plan) {
    for (const scope of SCOPES) {
      if (scopePrecedes(scope, level)) inheritedCount += ruleCounts.get(scope) ?? 0;
    }
  } else if (level !== "instance") {
    for (const { scope, rules: scoped } of inherited ?? []) {
      if (scopePrecedes(scope, level)) {
        inheritedCount += scoped.copy.length + scoped.hardlink.length + scoped.symlink.length;
      }
    }
  }
  const ownCount = ruleCounts.get(level) ?? 0;
  // A failed preview cannot vouch for "none": inherited rules may apply,
  // they just could not be resolved.
  if (failed) return `${ownCount} at this level · inherited unknown`;
  if (inheritedCount === 0 && ownCount === 0) return "none";
  return `${inheritedCount} inherited · ${ownCount} at this level`;
}

/** This level's patterns behind a conflict, per mode. */
export function conflictingPatternsOf(
  plan: ProvisioningPlan | null,
  level: ProvisioningScope,
): Record<ProvisioningMode, Set<string>> {
  const result: Record<ProvisioningMode, Set<string>> = {
    copy: new Set(),
    hardlink: new Set(),
    symlink: new Set(),
  };
  for (const conflict of plan?.conflicts ?? []) {
    if (conflict.scope !== level) continue;
    for (const rule of plan?.rules ?? []) {
      if (rule.scope === level && conflict.modes.includes(rule.mode) && rule.paths.includes(conflict.relative_path)) {
        result[rule.mode].add(rule.pattern);
      }
    }
  }
  return result;
}
