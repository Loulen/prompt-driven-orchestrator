import { useMemo } from "react";
import { pathsOverlap, scopePrecedes } from "../model/provisioning";
import type { ProvisioningPlan } from "../../shared/generated/ProvisioningPlan";
import { LEVEL_LABELS } from "../model/provisioningCopy";

type PlanRule = ProvisioningPlan["rules"][number];

// The "Resolved plan" footer of the Provisioning block, split out of
// `ProvisioningRulesEditor.tsx` (file size rule).

function overriddenOrigins(
  plan: ProvisioningPlan | null,
  rule: PlanRule,
): string[] {
  return (plan?.rules ?? [])
    .filter(
      (candidate) =>
        scopePrecedes(candidate.scope, rule.scope) &&
        candidate.mode !== rule.mode &&
        pathsOverlap(candidate.paths, rule.paths),
    )
    .map((candidate) => `${LEVEL_LABELS[candidate.scope]} ${candidate.mode}`)
    .filter((value, index, all) => all.indexOf(value) === index);
}

function overridingOrigins(
  plan: ProvisioningPlan | null,
  rule: PlanRule,
): string[] {
  return (plan?.rules ?? [])
    .filter(
      (candidate) =>
        scopePrecedes(rule.scope, candidate.scope) &&
        candidate.mode !== rule.mode &&
        pathsOverlap(candidate.paths, rule.paths),
    )
    .map((candidate) => `${LEVEL_LABELS[candidate.scope]} ${candidate.mode}`)
    .filter((value, index, all) => all.indexOf(value) === index);
}

export default function ProvisioningResolvedPlan({
  plan,
  frozen,
}: {
  plan: ProvisioningPlan | null;
  frozen: boolean;
}) {
  const gitProvidedPaths = useMemo(
    () =>
      new Set(
        (plan?.entries ?? [])
          .filter((entry) => entry.provided_by_git)
          .map((entry) => entry.relative_path),
      ),
    [plan],
  );
  return (
    <div className="border-t border-line px-3 py-2">
      <div className="mb-1 font-medium uppercase tracking-wide text-fg-4" style={{ fontSize: 9 }}>
        Resolved plan · {frozen ? "frozen" : "live"}
      </div>
      {(plan?.rules ?? []).filter((rule) => rule.unmatched).map((rule) => (
        <div key={`${rule.scope}-${rule.mode}-${rule.pattern}`} className="mb-1 rounded bg-amber-950/30 px-2 py-1 text-amber-400">
          No match: {rule.pattern} · normal, the Run still starts
        </div>
      ))}
      {(plan?.rules ?? []).map((rule) => {
        const overrides = overriddenOrigins(plan, rule);
        const overriddenBy = overridingOrigins(plan, rule);
        return (
          <details
            key={`${rule.scope}-${rule.mode}-${rule.pattern}`}
            className="border-t border-line-soft py-1 font-mono"
          >
            <summary className="cursor-pointer">
              {rule.pattern} · {LEVEL_LABELS[rule.scope]} · {rule.mode} ·{" "}
              {rule.paths.length + rule.excluded_paths.length}
              {overrides.length > 0 ? ` · overrides ${overrides.join(", ")}` : ""}
              {overriddenBy.length > 0
                ? ` · overridden by ${overriddenBy.join(", ")}`
                : ""}
            </summary>
            <div className="pl-4 text-fg-4">
              {rule.paths.map((path) => (
                <div key={path}>
                  {path}
                  {gitProvidedPaths.has(path) ? " · provided by Git · skipped" : ""}
                </div>
              ))}
              {rule.excluded_paths.map((excluded) => (
                <div key={`excluded-${excluded.relative_path}`} className="line-through">
                  {excluded.relative_path} · excluded by{" "}
                  {LEVEL_LABELS[excluded.excluded_by_scope]}
                </div>
              ))}
            </div>
          </details>
        );
      })}
      <div className="mt-1 text-fg-4">
        {plan?.entries.filter((entry) => !entry.provided_by_git).length ?? 0} paths added ·{" "}
        {plan?.entries.filter((entry) => entry.provided_by_git).length ?? 0} Git paths skipped
      </div>
    </div>
  );
}
