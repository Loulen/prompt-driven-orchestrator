import { useEffect, useMemo, useRef, useState } from "react";
import { ChevronDown, ChevronRight, Folder, FolderOpen, Sparkles, TriangleAlert, X } from "lucide-react";
import type { SkillBank, SkillRef, SkillTier } from "../types";
import { buildRows } from "../lib/skillTree";
import {
  addRefs,
  allSelected,
  effectiveCountLabel,
  removeRef,
  resolveEffectiveSkills,
  skillsInFolder,
  SKILL_TIER_LABEL,
  toRefs,
  type InheritedTier,
} from "../lib/skillSelection";

const EMPTY_BANK: SkillBank = { skills: [], folders: [], root_path: "" };

/**
 * The ONE skills selector (#669, ADR-0062), shared by the Configuration
 * d'instance, the Projet editor, the Run / Trigger creation and the node
 * inspector. Each tier shows its **own** skills (live checkboxes), the
 * **inherited** ones greyed with their origin tier, and the **effective total**
 * (strict additive union — no tier removes an inherited skill). Checking a
 * folder of the bank checks its skills *at this instant* (a gesture, never a
 * stored reference). An id the bank no longer knows is a warning row: the tier
 * keeps the id, the NodeRun runs without it.
 */
export default function SkillSelector({
  tier,
  own,
  onChange,
  inherited = [],
  bank = EMPTY_BANK,
  label = "Skills",
  testId = "skill-selector",
  readOnly = false,
}: {
  tier: SkillTier;
  own: SkillRef[];
  onChange: (skills: SkillRef[]) => void;
  inherited?: InheritedTier[];
  bank?: SkillBank;
  label?: string;
  testId?: string;
  readOnly?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const [filter, setFilter] = useState("");
  const rootRef = useRef<HTMLDivElement>(null);

  // Dismiss on outside click and Escape (#686, same pattern as AgentControl):
  // the picker is an absolute overlay that hides the fields below it, so it
  // must close on any gesture that is not aimed at it.
  useEffect(() => {
    if (!open) return;
    const onMouseDown = (event: MouseEvent) => {
      if (rootRef.current?.contains(event.target as Node)) return;
      setOpen(false);
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || event.defaultPrevented) return;
      setOpen(false);
    };
    document.addEventListener("mousedown", onMouseDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("mousedown", onMouseDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [open]);
  // Folders start expanded (every skill visible at a glance); `null` = untouched.
  const [collapsedOverride, setCollapsedOverride] = useState<Set<string> | null>(null);
  const expanded = useMemo(() => {
    const all = new Set(bank.folders.map((folder) => folder.id));
    if (!collapsedOverride) return all;
    for (const id of collapsedOverride) all.delete(id);
    return all;
  }, [bank, collapsedOverride]);

  const resolved = useMemo(
    () => resolveEffectiveSkills(tier, own, inherited, bank),
    [tier, own, inherited, bank],
  );
  const rows = useMemo(
    () => buildRows(bank.folders, bank.skills, expanded, filter),
    [bank, expanded, filter],
  );
  const inheritedById = useMemo(() => {
    const map = new Map<string, SkillTier[]>();
    for (const row of resolved.rows) {
      const others = row.tiers.filter((t) => t !== tier);
      if (others.length > 0) map.set(row.id, others);
    }
    return map;
  }, [resolved, tier]);
  const ownIds = useMemo(() => new Set(own.map((skill) => skill.id)), [own]);
  const hasMissing = resolved.missing.length > 0;
  const summary = resolved.rows
    .filter((row) => !row.missing)
    .map((row) => row.name)
    .join(", ");

  const toggleSkill = (id: string, name: string) => {
    if (readOnly) return;
    onChange(ownIds.has(id) ? removeRef(own, id) : addRefs(own, [{ id, name }]));
  };
  const toggleFolder = (folderId: string) => {
    if (readOnly) return;
    const skills = skillsInFolder(folderId, bank);
    if (skills.length === 0) return;
    if (allSelected(own, skills)) {
      const drop = new Set(skills.map((skill) => skill.id));
      onChange(own.filter((skill) => !drop.has(skill.id)));
    } else {
      onChange(addRefs(own, toRefs(skills)));
    }
  };
  const toggleExpanded = (folderId: string) => {
    setCollapsedOverride((prev) => {
      const next = new Set(prev ?? []);
      if (next.has(folderId)) next.delete(folderId);
      else next.add(folderId);
      return next;
    });
  };

  return (
    <div ref={rootRef} className="relative" data-testid={`${testId}-root`}>
      <span className="mb-1 block uppercase tracking-wider text-fg-4" style={{ fontSize: 9 }}>{label}</span>
      <button
        type="button"
        data-testid={testId}
        aria-expanded={open}
        disabled={readOnly}
        onClick={() => setOpen((value) => !value)}
        className={`flex w-full items-center gap-2 rounded border bg-bg-3 px-2 py-1.5 text-left ${
          hasMissing ? "border-st-blocked text-st-blocked" : "border-line-strong text-fg-2"
        } ${readOnly ? "cursor-default opacity-80" : ""}`}
      >
        {hasMissing ? <TriangleAlert size={11} className="shrink-0" /> : <Sparkles size={11} className="shrink-0" />}
        <span className="min-w-0 flex-1">
          <span className="block truncate font-medium" style={{ fontSize: 10.5 }} data-testid={`${testId}-count`}>
            {effectiveCountLabel(resolved.effectiveCount)}
          </span>
          <span className="block truncate font-mono text-fg-4" style={{ fontSize: 9.5 }}>
            {summary || "Pick skills from the bank"}
          </span>
        </span>
        {!readOnly && <ChevronDown size={11} className="shrink-0 text-fg-4" />}
      </button>

      {resolved.rows.length > 0 && (
        <ul className="mt-1 flex flex-col gap-0.5" data-testid={`${testId}-effective`}>
          {resolved.rows.map((row) => (
            <li
              key={row.id}
              data-testid={`${testId}-row-${row.id}`}
              data-own={row.own}
              data-inherited={row.inherited}
              data-missing={row.missing}
              data-tiers={row.tiers.join(" ")}
              className={`flex items-center gap-1.5 rounded px-1.5 py-0.5 ${
                row.missing ? "text-st-blocked" : row.own ? "text-fg-2" : "text-fg-4"
              }`}
              style={{ fontSize: 10 }}
            >
              {row.missing && <TriangleAlert size={10} className="shrink-0" />}
              <span className={`min-w-0 flex-1 truncate font-mono ${row.missing ? "line-through" : ""}`}>{row.name}</span>
              {row.tiers.map((t) => (
                <span
                  key={t}
                  data-tier={t}
                  className={`rounded border px-1 uppercase tracking-wider ${
                    t === tier ? "border-acc/60 text-acc" : "border-line text-fg-4"
                  }`}
                  style={{ fontSize: 8 }}
                  title={t === tier ? "Selected here" : `Inherited from the ${SKILL_TIER_LABEL[t].toLowerCase()} tier`}
                >
                  {SKILL_TIER_LABEL[t]}
                </span>
              ))}
              {row.own && !readOnly && (
                <button
                  type="button"
                  aria-label={`Remove ${row.name}`}
                  data-testid={`${testId}-remove-${row.id}`}
                  onClick={() => onChange(removeRef(own, row.id))}
                  className="shrink-0 rounded p-0.5 text-fg-4 hover:text-fg-2"
                >
                  <X size={10} />
                </button>
              )}
            </li>
          ))}
        </ul>
      )}
      {hasMissing && (
        <p className="mt-1 text-st-blocked" style={{ fontSize: 9.5 }} data-testid={`${testId}-missing`}>
          {resolved.missing.length === 1
            ? `Skill ${resolved.missing[0].name} no longer exists in the bank. It is skipped; runs still start.`
            : `${resolved.missing.length} selected skills no longer exist in the bank. They are skipped; runs still start.`}
        </p>
      )}

      {open && !readOnly && (
        <div
          role="dialog"
          data-testid={`${testId}-popover`}
          className="absolute left-0 z-40 mt-1 w-full min-w-[280px] rounded border border-line-strong bg-bg-4 p-1.5 shadow-xl"
        >
          <input
            type="search"
            value={filter}
            onChange={(event) => setFilter(event.target.value)}
            placeholder="Filter skills…"
            data-testid={`${testId}-filter`}
            className="mb-1 w-full rounded border border-line bg-bg-3 px-2 py-1 text-fg outline-none focus:border-acc"
            style={{ fontSize: 10.5 }}
          />
          {bank.skills.length === 0 ? (
            <p className="px-1 py-2 text-fg-4" style={{ fontSize: 10 }} data-testid={`${testId}-empty`}>
              The bank is empty. Add skills in Settings → Skills.
            </p>
          ) : (
            <ul className="max-h-64 overflow-y-auto" data-testid={`${testId}-tree`}>
              {rows.map((row) => {
                if (row.ref.kind === "folder" && row.folder) {
                  const skills = skillsInFolder(row.folder.id, bank);
                  const checked = allSelected(own, skills);
                  const some = !checked && skills.some((skill) => ownIds.has(skill.id));
                  return (
                    <li
                      key={`f-${row.folder.id}`}
                      className="flex items-center gap-1.5 rounded px-1 py-0.5 hover:bg-bg-3"
                      style={{ paddingLeft: 4 + row.depth * 12, fontSize: 10.5 }}
                      data-testid={`${testId}-folder-${row.folder.id}`}
                    >
                      <input
                        type="checkbox"
                        aria-label={`Select every skill of ${row.folder.name}`}
                        data-testid={`${testId}-folder-check-${row.folder.id}`}
                        checked={checked}
                        ref={(el) => {
                          if (el) el.indeterminate = some;
                        }}
                        disabled={skills.length === 0}
                        onChange={() => toggleFolder(row.folder!.id)}
                        className="accent-acc"
                      />
                      <button
                        type="button"
                        onClick={() => toggleExpanded(row.folder!.id)}
                        className="flex min-w-0 flex-1 items-center gap-1 text-left text-fg-2"
                        aria-expanded={row.expanded}
                      >
                        {row.expanded ? <ChevronDown size={10} /> : <ChevronRight size={10} />}
                        {row.expanded ? <FolderOpen size={11} className="text-fg-4" /> : <Folder size={11} className="text-fg-4" />}
                        <span className="truncate">{row.folder.name}</span>
                        <span className="text-fg-4" style={{ fontSize: 9 }}>{row.count}</span>
                      </button>
                    </li>
                  );
                }
                if (row.ref.kind === "skill" && row.skill) {
                  const skill = row.skill;
                  const isOwn = ownIds.has(skill.id);
                  const from = inheritedById.get(skill.id);
                  return (
                    <li
                      key={`s-${skill.id}`}
                      className="flex items-center gap-1.5 rounded px-1 py-0.5 hover:bg-bg-3"
                      style={{ paddingLeft: 4 + row.depth * 12, fontSize: 10.5 }}
                      data-testid={`${testId}-option-${skill.id}`}
                    >
                      <input
                        type="checkbox"
                        aria-label={skill.name}
                        data-testid={`${testId}-check-${skill.id}`}
                        checked={isOwn || !!from}
                        disabled={!isOwn && !!from}
                        onChange={() => toggleSkill(skill.id, skill.name)}
                        className="accent-acc"
                        title={from && !isOwn ? `Inherited from the ${from.map((t) => SKILL_TIER_LABEL[t].toLowerCase()).join(", ")} tier` : undefined}
                      />
                      <span className={`min-w-0 flex-1 truncate font-mono ${from && !isOwn ? "text-fg-4" : "text-fg"}`}>
                        {skill.name}
                      </span>
                      {from && !isOwn && (
                        <span className="rounded border border-line px-1 uppercase tracking-wider text-fg-4" style={{ fontSize: 8 }}>
                          {from.map((t) => SKILL_TIER_LABEL[t]).join(" · ")}
                        </span>
                      )}
                    </li>
                  );
                }
                return null;
              })}
            </ul>
          )}
          <p className="mt-1 border-t border-line px-1 pt-1 text-fg-4" style={{ fontSize: 9 }}>
            Inherited skills are greyed and cannot be removed here (additive union). Checking a folder checks its skills now.
          </p>
        </div>
      )}
    </div>
  );
}
