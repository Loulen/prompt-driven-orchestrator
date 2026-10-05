import { useEffect, useId, useMemo, useRef, useState } from "react";
import type { ReactNode } from "react";
import { ChevronDown, ChevronRight } from "lucide-react";
import { previewProvisioning } from "../api/provisioning";
import type { ProvisioningMode } from "../../shared/generated/ProvisioningMode";
import type { ProvisioningPlan } from "../../shared/generated/ProvisioningPlan";
import type { ProvisioningRules } from "../../shared/generated/ProvisioningRules";
import type { ProvisioningScope } from "../../shared/generated/ProvisioningScope";
import type { ScopedProvisioningRules } from "../../shared/generated/ScopedProvisioningRules";
import { ruleLines as lines, SCOPES, scopePrecedes } from "../model/provisioning";
import {
  LEVEL_LABELS,
  LEVELS_HINT,
  MODES,
  PROVISIONING_CONFLICT_REASON,
  SUBTITLE,
} from "../model/provisioningCopy";
import InfoHint from "./InfoHint";
import ProvisioningResolvedPlan from "./ProvisioningResolvedPlan";

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
  defaultExpanded = false,
  leading,
  trailing,
}: {
  level: ProvisioningScope;
  repository: string;
  rules: ProvisioningRules;
  onChange: (rules: ProvisioningRules) => void;
  /** `reason` is set whenever `valid` is false, for the host to explain a
   *  blocked action (New Run's Launch). */
  onValidityChange?: (valid: boolean, reason?: string) => void;
  readOnly?: boolean;
  frozenAt?: string;
  frozenPlan?: ProvisioningPlan;
  inherited?: ScopedProvisioningRules[];
  gitRef?: string;
  /** Notion #7: collapsed everywhere provisioning is secondary; Settings, where
   *  the section *is* provisioning, opens it. Never remembered across mounts. */
  defaultExpanded?: boolean;
  /** Rendered at the top of the expanded body (e.g. a "Resolve against" input). */
  leading?: ReactNode;
  /** Rendered at the bottom of the expanded body (e.g. a Save row). */
  trailing?: ReactNode;
}) {
  const [plan, setPlan] = useState<ProvisioningPlan | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [expanded, setExpanded] = useState(defaultExpanded);
  const bodyId = useId();
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
      if (frozenPlan.conflicts.length === 0) onValidityChange?.(true);
      else onValidityChange?.(false, PROVISIONING_CONFLICT_REASON);
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
          if (next.conflicts.length === 0) {
            onValidityChange?.(true);
          } else {
            setExpanded(true);
            onValidityChange?.(false, PROVISIONING_CONFLICT_REASON);
          }
        })
        .catch((reason: unknown) => {
          if (cancelled) return;
          const message = reason instanceof Error ? reason.message : "Preview failed";
          setPlan(null);
          setError(message);
          setExpanded(true);
          onValidityChange?.(false, `Provisioning preview failed: ${message}`);
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

  // The preview keeps running while collapsed, so a conflict or a failed
  // preview is known without opening the block — and holds it open: a launch
  // is never blocked by something hidden. The preview callbacks also latch
  // `expanded`, so fixing the conflict does not snap the block shut under the
  // user's cursor. A click on the header while blocked is ignored, not stored:
  // stored, it would replay as a collapse the moment the conflict is fixed.
  const blocked = (visiblePlan?.conflicts.length ?? 0) > 0 || !!visibleError;
  const open = expanded || blocked;

  // The header marker; dropped when the expanded body carries its own
  // "Resolve against" field (`leading`), which would repeat it just below.
  const marker = frozenAt
    ? `🔒 frozen at ${frozenAt} · reused on restart`
    : open && leading
      ? null
      : `Resolve against ${repository || "a repository"}`;

  const summary = useMemo(() => {
    let inheritedCount = 0;
    if (visiblePlan) {
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
    if (visibleError) return `${ownCount} at this level · inherited unknown`;
    if (inheritedCount === 0 && ownCount === 0) return "none";
    return `${inheritedCount} inherited · ${ownCount} at this level`;
  }, [visiblePlan, visibleError, level, ruleCounts, inherited]);

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

  return (
    <section
      className="@container rounded-md border border-line bg-bg-3"
      data-testid={`provisioning-${level}`}
    >
      <div className={`px-3 py-2 ${open ? "border-b border-line" : ""}`}>
        <div className="flex items-center justify-between gap-2">
          <button
            type="button"
            aria-expanded={open}
            aria-controls={bodyId}
            onClick={() => {
              if (!blocked) setExpanded(!open);
            }}
            title={blocked ? "Fix the provisioning issue to collapse" : undefined}
            className="flex max-w-full shrink-0 cursor-pointer items-center gap-1 text-left font-medium text-fg"
            data-testid="provisioning-toggle"
          >
            {open ? (
              <ChevronDown size={12} aria-hidden="true" className="shrink-0 text-fg-4" />
            ) : (
              <ChevronRight size={12} aria-hidden="true" className="shrink-0 text-fg-4" />
            )}
            <span className="truncate">
              Provisioning <span className="font-normal text-fg-4">· {summary}</span>
            </span>
          </button>
          {marker && (
            <div className="min-w-0 truncate text-right text-fg-4" style={{ fontSize: 10 }} title={marker}>
              {marker}
            </div>
          )}
        </div>
        <div className="mt-0.5 text-fg-4" style={{ fontSize: 10 }}>
          {SUBTITLE}
        </div>
      </div>
      {open && (
        <div id={bodyId} data-testid="provisioning-body">
          {leading && <div className="border-b border-line px-3 py-2">{leading}</div>}
          <div className="flex items-center gap-4 border-b border-line px-3 py-1.5 text-fg-4">
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
            <InfoHint label="About provisioning levels" content={LEVELS_HINT} />
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
            {MODES.map(({ key, color, hint }) => (
              <div key={key} className="overflow-hidden rounded border border-line">
                <div className="flex items-center gap-1.5 border-b border-line px-2 py-1.5 font-medium capitalize">
                  <span className={`h-1.5 w-1.5 rounded-sm ${color}`} />
                  {key}
                  <InfoHint label={`About ${key}`} content={hint} />
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

          <ProvisioningResolvedPlan
            plan={visiblePlan}
            frozen={!!frozenAt}
          />
          {trailing && <div className="border-t border-line px-3 py-2">{trailing}</div>}
        </div>
      )}
    </section>
  );
}
