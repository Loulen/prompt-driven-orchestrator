import { useEffect, useMemo, useRef, useState } from "react";
import { ChevronDown, ChevronRight, Folder, FolderOpen, Sparkles, TriangleAlert } from "lucide-react";
import type { SkillBank, SkillRef, SkillTier } from "../types";
import { buildRows } from "../lib/skillTree";
import {
  activeCountLabel,
  addRefs,
  allSelected,
  removeRef,
  resolveActiveSkills,
  skillsInFolder,
  SKILL_TIER_LABEL,
  toRefs,
  type ActiveRow,
  type InheritedTier,
} from "../lib/skillSelection";

const EMPTY_BANK: SkillBank = { skills: [], folders: [], root_path: "" };

/**
 * The ONE skills selector (#669, ADR-0062), shared by the Configuration
 * d'instance, the Projet editor, the Run / Trigger creation and the node
 * inspector. Since #849 it is **compact**: folded, one row — the icon, the
 * label, and a discreet sub-line with the **count alone** (« n active skills »),
 * never the names. The permanent list of names under the button is gone: it only
 * doubled what the popover already says, four screens paid its height for it, and
 * a selection of any size overflowed it.
 *
 * Unfolded, the bank's tree IS the reading: a checked box is a selected skill, an
 * inherited one is checked, greyed and locked with its origin tier beside it
 * (strict additive union — no tier removes an inherited skill). Checking a folder
 * checks its skills *at this instant* (a gesture, never a stored reference). An id
 * the bank no longer knows is struck through at the head of the popover, still
 * checked and still removable there; folded, it becomes the alert icon on the
 * right of the button, its sentence in the tooltip.
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
    () => resolveActiveSkills(tier, own, inherited, bank),
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
  const missingMessage = missingSentence(resolved.missing);

  // A pick leaves the picker OPEN (#849). It closed on every tick since #837,
  // when the list under the button still said what had been picked; that list is
  // gone, so the popover is now the only place the selection can be read — and
  // ticking two skills in a row is the ordinary gesture (the *First run* tour
  // asks for exactly that). Escape, an outside click and the button itself all
  // still fold it away.
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
      {/* One row, label included: the caption above the button was a third line
          for a control that says two things (#849). */}
      <button
        type="button"
        data-testid={testId}
        aria-expanded={open}
        onClick={() => setOpen((value) => !value)}
        className={`flex w-full items-center gap-2 rounded border bg-bg-3 px-2 py-1.5 text-left ${
          hasMissing ? "border-st-blocked" : "border-line-strong"
        } text-fg-2 ${readOnly ? "opacity-80" : ""}`}
      >
        <Sparkles size={11} className="shrink-0" />
        <span className="min-w-0 flex-1">
          <span className="block truncate font-medium" style={{ fontSize: 10.5 }}>{label}</span>
          <span className="block truncate text-fg-4" style={{ fontSize: 9.5 }} data-testid={`${testId}-count`}>
            {activeCountLabel(resolved.activeCount)}
          </span>
        </span>
        {hasMissing && (
          // The sentence is one hover away instead of a red paragraph under the
          // button — the same words, none of the height (#849).
          <span
            data-testid={`${testId}-alert`}
            title={missingMessage}
            // `role="img"` so the sentence is a NAME, not an inert attribute on a
            // bare span: a reader who never hovers still gets the warning.
            role="img"
            aria-label={missingMessage}
            className="shrink-0 text-st-blocked"
          >
            <TriangleAlert size={11} />
          </span>
        )}
        <ChevronDown size={11} className="shrink-0 text-fg-4" />
      </button>

      {/* Read-only (a node whose spawn froze its skills, a Trigger's detail) opens
          the popover all the same: the reading is the point, and refusing to open
          left a count nobody could expand (#849). Only the gestures are frozen. */}
      {open && (
        <div
          role="dialog"
          data-testid={`${testId}-popover`}
          className="absolute left-0 z-40 mt-1 w-full min-w-[280px] rounded border border-line-strong bg-bg-4 p-1.5 shadow-xl"
        >
          {hasMissing && (
            <ul className="mb-1 flex flex-col gap-0.5 border-b border-line pb-1" data-testid={`${testId}-missing`}>
              {resolved.missing.map((row) => {
                // `own` = this tier is one of the tiers that selected it, so the
                // uncheck below has something of ours to remove.
                const isOwn = row.own;
                return (
                  <li
                    key={`m-${row.id}`}
                    data-testid={`${testId}-missing-${row.id}`}
                    data-own={isOwn}
                    className="flex items-center gap-1.5 rounded px-1 py-0.5 text-st-blocked"
                    style={{ fontSize: 10.5 }}
                  >
                    <input
                      type="checkbox"
                      aria-label={row.name}
                      data-testid={`${testId}-check-${row.id}`}
                      checked
                      // Only this tier's own reference can be dropped here; one a
                      // coarser tier still selects is a fact, not a choice.
                      disabled={readOnly || !isOwn}
                      onChange={() => {
                        if (readOnly) return;
                        onChange(removeRef(own, row.id));
                      }}
                      className="accent-acc"
                    />
                    <TriangleAlert size={10} className="shrink-0" />
                    <span className={`min-w-0 flex-1 truncate font-mono line-through ${isOwn ? "" : "text-fg-4"}`}>
                      {row.name}
                    </span>
                    {!isOwn &&
                      row.tiers.map((t) => (
                        <span
                          key={t}
                          data-tier={t}
                          className="rounded border border-line px-1 uppercase tracking-wider text-fg-4"
                          style={{ fontSize: 8 }}
                        >
                          {SKILL_TIER_LABEL[t]}
                        </span>
                      ))}
                  </li>
                );
              })}
            </ul>
          )}
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
                        disabled={readOnly || skills.length === 0}
                        onChange={() => toggleFolder(row.folder!.id)}
                        className="accent-acc"
                      />
                      {/* The chevron stays live in read-only: folding a folder
                          reads the tree, it changes no selection. */}
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
                  const locked = readOnly || (!isOwn && !!from);
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
                        disabled={locked}
                        onChange={() => toggleSkill(skill.id, skill.name)}
                        className="accent-acc"
                        title={from && !isOwn ? `Inherited from the ${from.map((t) => SKILL_TIER_LABEL[t].toLowerCase()).join(", ")} tier` : undefined}
                      />
                      {/* The name is the target everyone aims at — a picker whose
                          rows only answer on the 14-pixel checkbox reads as broken
                          (#824). Same gesture, same disabled rule as the box. */}
                      <button
                        type="button"
                        tabIndex={-1}
                        disabled={locked}
                        onClick={() => toggleSkill(skill.id, skill.name)}
                        data-testid={`${testId}-label-${skill.id}`}
                        className={`min-w-0 flex-1 truncate text-left font-mono disabled:cursor-default ${from && !isOwn ? "text-fg-4" : "text-fg"}`}
                      >
                        {skill.name}
                      </button>
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
            {readOnly
              ? "Frozen: this selection is read-only here."
              : "Inherited skills are greyed and cannot be removed here (additive union). Checking a folder checks its skills now."}
          </p>
        </div>
      )}
    </div>
  );
}

/** The sentence the alert icon carries — unchanged words, now a tooltip (#849). */
function missingSentence(missing: ActiveRow[]): string {
  if (missing.length === 1) {
    return `Skill ${missing[0].name} no longer exists in the bank. It is skipped; runs still start.`;
  }
  return `${missing.length} selected skills no longer exist in the bank. They are skipped; runs still start.`;
}
