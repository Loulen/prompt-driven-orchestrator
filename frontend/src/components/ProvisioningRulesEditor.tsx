import { useEffect, useMemo, useRef, useState } from "react";
import { previewProvisioning } from "../api";
import type {
  ProvisioningMode,
  ProvisioningPlan,
  ProvisioningRules,
  ProvisioningScope,
  ScopedProvisioningRules,
} from "../types";

const LEVEL_LABELS: Record<ProvisioningScope, string> = {
  instance: "Instance",
  project: "Project",
  run: "Run",
  isolated_node: "Node",
};

const MODES: Array<{ key: ProvisioningMode; color: string }> = [
  { key: "copy", color: "bg-emerald-400" },
  { key: "hardlink", color: "bg-blue-400" },
  { key: "symlink", color: "bg-violet-400" },
];

const SCOPES: ProvisioningScope[] = [
  "instance",
  "project",
  "run",
  "isolated_node",
];

function scopePrecedes(left: ProvisioningScope, right: ProvisioningScope): boolean {
  return SCOPES.indexOf(left) < SCOPES.indexOf(right);
}

function pathsOverlap(left: string[], right: string[]): boolean {
  return left.some((leftPath) =>
    right.some(
      (rightPath) =>
        leftPath === rightPath ||
        leftPath.startsWith(`${rightPath}/`) ||
        rightPath.startsWith(`${leftPath}/`),
    ),
  );
}

function lines(value: string): string[] {
  return value
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean);
}

export default function ProvisioningRulesEditor({
  level,
  repository,
  rules,
  onChange,
  onValidityChange,
  readOnly = false,
  frozenAt,
  frozenPlan,
  inherited,
  gitRef = "HEAD",
}: {
  level: ProvisioningScope;
  repository: string;
  rules: ProvisioningRules;
  onChange: (rules: ProvisioningRules) => void;
  onValidityChange?: (valid: boolean) => void;
  readOnly?: boolean;
  frozenAt?: string;
  frozenPlan?: ProvisioningPlan;
  inherited?: ScopedProvisioningRules[];
  gitRef?: string;
}) {
  const [plan, setPlan] = useState<ProvisioningPlan | null>(null);
  const [error, setError] = useState<string | null>(null);
  const textareas = useRef<Partial<Record<ProvisioningMode, HTMLTextAreaElement>>>({});
  const serialized = useMemo(() => JSON.stringify(rules), [rules]);
  const visiblePlan = frozenPlan ?? (repository.trim() ? plan : null);
  const visibleError = frozenPlan ? null : repository.trim() ? error : null;
  const previewInherited = useMemo(
    () => (level === "instance" ? [] : inherited),
    [level, inherited],
  );

  useEffect(() => {
    if (frozenPlan) {
      onValidityChange?.(frozenPlan.conflicts.length === 0);
      return;
    }
    if (!repository.trim()) {
      onValidityChange?.(true);
      return;
    }
    let cancelled = false;
    const timer = window.setTimeout(() => {
      previewProvisioning(repository, level, rules, previewInherited, gitRef)
        .then((next) => {
          if (cancelled) return;
          setPlan(next);
          setError(null);
          onValidityChange?.(next.conflicts.length === 0);
        })
        .catch((reason: unknown) => {
          if (cancelled) return;
          setPlan(null);
          setError(reason instanceof Error ? reason.message : "Preview failed");
          onValidityChange?.(false);
        });
    }, 150);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [
    repository,
    serialized,
    level,
    onValidityChange,
    rules,
    previewInherited,
    gitRef,
    frozenPlan,
  ]);

  const counts = useMemo(() => {
    const result: Record<ProvisioningMode, Map<string, number>> = {
      copy: new Map(),
      hardlink: new Map(),
      symlink: new Map(),
    };
    for (const rule of visiblePlan?.rules ?? []) {
      result[rule.mode].set(
        rule.pattern,
        rule.paths.length + rule.excluded_paths.length,
      );
    }
    return result;
  }, [visiblePlan]);

  const ruleCounts = useMemo(() => {
    const result = new Map<ProvisioningScope, number>();
    for (const scope of SCOPES) {
      result.set(
        scope,
        (visiblePlan?.rules ?? []).filter((rule) => rule.scope === scope).length,
      );
    }
    if (!visiblePlan) {
      result.set(level, rules.copy.length + rules.hardlink.length + rules.symlink.length);
    }
    return result;
  }, [level, visiblePlan, rules]);

  const conflictingPatterns = useMemo(() => {
    const result: Record<ProvisioningMode, Set<string>> = {
      copy: new Set(),
      hardlink: new Set(),
      symlink: new Set(),
    };
    for (const conflict of visiblePlan?.conflicts ?? []) {
      if (conflict.scope !== level) continue;
      for (const rule of visiblePlan?.rules ?? []) {
        if (
          rule.scope === level &&
          conflict.modes.includes(rule.mode) &&
          rule.paths.includes(conflict.relative_path)
        ) {
          result[rule.mode].add(rule.pattern);
        }
      }
    }
    return result;
  }, [level, visiblePlan]);

  const gitProvidedPaths = useMemo(
    () =>
      new Set(
        (visiblePlan?.entries ?? [])
          .filter((entry) => entry.provided_by_git)
          .map((entry) => entry.relative_path),
      ),
    [visiblePlan],
  );

  function update(mode: ProvisioningMode, text: string) {
    onChange({ ...rules, [mode]: lines(text) });
  }

  function jumpToConflict(mode: ProvisioningMode, relativePath: string) {
    const textarea = textareas.current[mode];
    const rule = visiblePlan?.rules.find(
      (candidate) =>
        candidate.scope === level &&
        candidate.mode === mode &&
        candidate.paths.includes(relativePath),
    );
    if (!textarea || !rule) return;
    const lineIndex = rules[mode].indexOf(rule.pattern);
    if (lineIndex < 0) return;
    const start = rules[mode]
      .slice(0, lineIndex)
      .reduce((length, line) => length + line.length + 1, 0);
    textarea.focus();
    textarea.setSelectionRange(start, start + rule.pattern.length);
  }

  function overriddenOrigins(
    rule: NonNullable<ProvisioningPlan["rules"]>[number],
  ): string[] {
    return (visiblePlan?.rules ?? [])
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
    rule: NonNullable<ProvisioningPlan["rules"]>[number],
  ): string[] {
    return (visiblePlan?.rules ?? [])
      .filter(
        (candidate) =>
          scopePrecedes(rule.scope, candidate.scope) &&
          candidate.mode !== rule.mode &&
          pathsOverlap(candidate.paths, rule.paths),
      )
      .map((candidate) => `${LEVEL_LABELS[candidate.scope]} ${candidate.mode}`)
      .filter((value, index, all) => all.indexOf(value) === index);
  }

  return (
    <section
      className="@container rounded-md border border-line bg-bg-3"
      data-testid={`provisioning-${level}`}
    >
      <div className="flex items-center justify-between border-b border-line px-3 py-2">
        <div className="font-medium text-fg">Provisioning</div>
        <div className="text-fg-4" style={{ fontSize: 10 }}>
          {frozenAt ? `🔒 frozen at ${frozenAt} · reused on restart` : `Resolve against ${repository || "a repository"}`}
        </div>
      </div>
      <div className="flex gap-4 border-b border-line px-3 py-1.5 text-fg-4">
        {SCOPES.map(
          (scope) => (
            <span
              key={scope}
              className={scope === level ? "border-b-2 border-acc pb-1 text-fg" : ""}
            >
              {LEVEL_LABELS[scope]} · {ruleCounts.get(scope) ?? 0}
            </span>
          ),
        )}
      </div>

      {visiblePlan?.conflicts.map((conflict) => (
        <div
          key={`${conflict.scope}-${conflict.relative_path}`}
          role="alert"
          className="m-2 rounded border border-st-failed bg-red-950/30 px-2 py-1.5 text-st-failed"
        >
          Mode conflict in {LEVEL_LABELS[conflict.scope]} — {conflict.relative_path} is
          declared as {conflict.modes.join(" and ")}. Keep one.{" "}
          {conflict.scope === level &&
            conflict.modes.map((mode) => (
              <button
                key={mode}
                type="button"
                aria-label={`Jump to ${mode} rule for ${conflict.relative_path}`}
                onClick={() => jumpToConflict(mode, conflict.relative_path)}
                className="ml-1 underline underline-offset-2"
              >
                Jump to {mode}
              </button>
            ))}
        </div>
      ))}
      {visibleError && <div role="alert" className="m-2 text-st-failed">{visibleError}</div>}

      <div
        className="grid grid-cols-1 gap-2 p-2 @[520px]:grid-cols-3"
        data-testid="provisioning-mode-grid"
      >
        {MODES.map(({ key, color }) => (
          <div key={key} className="overflow-hidden rounded border border-line">
            <div className="flex items-center gap-1.5 border-b border-line px-2 py-1.5 font-medium capitalize">
              <span className={`h-1.5 w-1.5 rounded-sm ${color}`} />
              {key}
            </div>
            <div className="space-y-1 border-b border-line bg-bg-2 px-2 py-1.5">
              {(visiblePlan?.rules ?? [])
                .filter(
                  (rule) =>
                    rule.mode === key &&
                    scopePrecedes(rule.scope, level),
                )
                .map((rule) => {
                  const redeclared = MODES.some(({ key: mode }) =>
                    rules[mode].includes(rule.pattern),
                  );
                  return (
                    <div
                      key={`${rule.scope}-${rule.pattern}`}
                      className={`flex items-center justify-between font-mono text-fg-4 ${redeclared ? "line-through opacity-60" : ""}`}
                      style={{
                        fontSize: 9,
                        textDecorationLine: redeclared ? "line-through" : "none",
                      }}
                    >
                      <span className="truncate">{rule.pattern}</span>
                      <span className="ml-1 shrink-0 rounded bg-bg-4 px-1">
                        {LEVEL_LABELS[rule.scope]} · {rule.paths.length + rule.excluded_paths.length}
                      </span>
                    </div>
                  );
                })}
              {!visiblePlan?.rules.some(
                (rule) =>
                  rule.mode === key &&
                  scopePrecedes(rule.scope, level),
              ) && (
                <div className="text-fg-4" style={{ fontSize: 9 }}>No inherited rules</div>
              )}
            </div>
            <div className="relative">
              <textarea
                ref={(element) => {
                  if (element) textareas.current[key] = element;
                  else delete textareas.current[key];
                }}
                aria-label={`${key[0].toUpperCase()}${key.slice(1)} patterns`}
                value={rules[key].join("\n")}
                onChange={(event) => update(key, event.target.value)}
                readOnly={readOnly}
                aria-invalid={conflictingPatterns[key].size > 0}
                rows={5}
                className={`w-full resize-y bg-bg-2 p-2 pr-14 font-mono text-fg outline-none ${conflictingPatterns[key].size ? "ring-1 ring-inset ring-st-failed" : ""}`}
                placeholder={"one pattern per line\n!path excludes"}
                style={{ fontSize: 10 }}
              />
              <div className="pointer-events-none absolute right-2 top-2 space-y-[2px] text-right font-mono text-fg-4" style={{ fontSize: 10 }}>
                {rules[key].map((pattern) => (
                  <div
                    key={pattern}
                    className={conflictingPatterns[key].has(pattern) ? "text-st-failed" : ""}
                  >
                    {conflictingPatterns[key].has(pattern) ? "conflict" : (counts[key].get(pattern) ?? "·")}
                  </div>
                ))}
              </div>
            </div>
          </div>
        ))}
      </div>

      <div className="border-t border-line px-3 py-2">
        <div className="mb-1 font-medium uppercase tracking-wide text-fg-4" style={{ fontSize: 9 }}>
          Resolved plan · {frozenAt ? "frozen" : "live"}
        </div>
        {(visiblePlan?.rules ?? []).filter((rule) => rule.unmatched).map((rule) => (
          <div key={`${rule.scope}-${rule.mode}-${rule.pattern}`} className="mb-1 rounded bg-amber-950/30 px-2 py-1 text-amber-400">
            No match: {rule.pattern} · normal, the Run still starts
          </div>
        ))}
        {(visiblePlan?.rules ?? []).map((rule) => {
          const overrides = overriddenOrigins(rule);
          const overriddenBy = overridingOrigins(rule);
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
          {visiblePlan?.entries.filter((entry) => !entry.provided_by_git).length ?? 0} paths added ·{" "}
          {visiblePlan?.entries.filter((entry) => entry.provided_by_git).length ?? 0} Git paths skipped
        </div>
      </div>
    </section>
  );
}
