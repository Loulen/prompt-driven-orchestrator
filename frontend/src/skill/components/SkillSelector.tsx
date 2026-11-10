import { useEffect, useMemo, useRef, useState } from "react";
import { ChevronDown, ChevronRight, Folder, FolderOpen, Sparkles, TriangleAlert } from "lucide-react";
import type { SkillBank } from "../model/skill";
import { AnchoredPanel } from "../../shared/ui/kit/layer";
import { registerTransientOverlay } from "../../lib/overlays";
import type { SkillRef } from "../../shared/generated/SkillRef";
import type { SkillTier } from "../../shared/generated/SkillTier";
import { buildRows } from "../model/skillTree";
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
} from "../model/skillSelection";

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
  showLabel = true,
  testId = "skill-selector",
  readOnly = false,
  added = [],
}: {
  tier: SkillTier;
  own: SkillRef[];
  onChange: (skills: SkillRef[]) => void;
  inherited?: InheritedTier[];
  bank?: SkillBank;
  label?: string;
  /** The label inside the button; off in a label-left form row, where it reads « Skills ». */
  showLabel?: boolean;
  testId?: string;
  readOnly?: boolean;
  /** Skills a prompt's `/` completion just added to `own`: a chip each under
   *  the button, « <name> · added », while they stay selected. */
  added?: SkillRef[];
}) {
  const [open, setOpen] = useState(false);
  const [filter, setFilter] = useState("");
  const triggerRef = useRef<HTMLButtonElement>(null);
  const popoverRef = useRef<HTMLDivElement>(null);
  // Open, it is a picker someone else may need folded: a guided tour moving on
  // to the Prompt must not leave it over the lit field (FP of CORE-227).
  useEffect(() => (open ? registerTransientOverlay(() => setOpen(false), () => popoverRef.current) : undefined), [open]);

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
  const addedChips = added.filter((skill) => ownIds.has(skill.id));
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
    <div className="ln-skills" data-testid={`${testId}-root`}>
      {/* One row, label included (#849): the label, the count at the end. */}
      <button
        ref={triggerRef}
        type="button"
        data-testid={testId}
        aria-expanded={open}
        aria-label={showLabel ? undefined : label}
        aria-invalid={hasMissing || undefined}
        onClick={() => setOpen((value) => !value)}
        className="d-sel ln-skills__field"
        data-readonly={readOnly || undefined}
      >
        <Sparkles className="d-sel__lead" size={13} strokeWidth={1.5} aria-hidden />
        <span className="d-sel__val">{showLabel ? label : "Skills"}</span>
        <span className="ln-sel__count" data-testid={`${testId}-count`}>
          {activeCountLabel(resolved.activeCount)}
        </span>
        {hasMissing && (
          // The sentence is one hover away instead of a red paragraph under the
          // button (#849); `role="img"` makes it a name a reader gets.
          <span
            data-testid={`${testId}-alert`}
            title={missingMessage}
            role="img"
            aria-label={missingMessage}
            className="ln-skills__alert"
          >
            <TriangleAlert size={12} strokeWidth={1.5} aria-hidden />
          </span>
        )}
        <ChevronDown className="d-sel__chev" size={12} strokeWidth={1.5} aria-hidden />
      </button>

      {addedChips.length > 0 && (
        <ul className="ln-chips" data-testid={`${testId}-added`}>
          {addedChips.map((skill) => (
            <li key={skill.id} data-testid={`${testId}-added-${skill.id}`} className="d-chip d-chip--mono ln-skills__added">
              {bank.skills.find((known) => known.id === skill.id)?.name ?? skill.name} · added
            </li>
          ))}
        </ul>
      )}

      {/* Read-only (a node whose spawn froze its skills, a Trigger's detail) opens
          the panel all the same: the reading is the point (#849). Only the
          gestures are frozen. */}
      <AnchoredPanel anchor={triggerRef} open={open} onClose={() => setOpen(false)} className="d-pop--panel ln-skills__pop">
        <div
          ref={popoverRef}
          role="dialog"
          aria-label={label}
          data-testid={`${testId}-popover`}
        >
          {hasMissing && (
            <ul className="ln-skills__missing" data-testid={`${testId}-missing`}>
              {resolved.missing.map((row) => {
                // `own` = this tier is one of the tiers that selected it, so the
                // uncheck below has something of ours to remove.
                const isOwn = row.own;
                return (
                  <li
                    key={`m-${row.id}`}
                    data-testid={`${testId}-missing-${row.id}`}
                    data-own={isOwn}
                    className="ln-skills__row ln-skills__row--missing"
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
                    />
                    <TriangleAlert size={12} strokeWidth={1.5} aria-hidden />
                    <span className="ln-skills__name ln-skills__name--gone" data-own={isOwn}>
                      {row.name}
                    </span>
                    {!isOwn &&
                      row.tiers.map((t) => (
                        <span key={t} data-tier={t} className="d-chip ln-skills__tier">
                          {SKILL_TIER_LABEL[t]}
                        </span>
                      ))}
                  </li>
                );
              })}
            </ul>
          )}
          <label className="d-pop__search">
            <input
              type="search"
              value={filter}
              onChange={(event) => setFilter(event.target.value)}
              placeholder="Filter skills"
              aria-label="Filter skills"
              spellCheck={false}
              autoFocus
              data-testid={`${testId}-filter`}
            />
          </label>
          {bank.skills.length === 0 ? (
            <p className="d-pop__empty" data-testid={`${testId}-empty`}>
              No skill in the bank
            </p>
          ) : (
            <ul className="d-pop__list" data-testid={`${testId}-tree`}>
              {rows.map((row) => {
                if (row.ref.kind === "folder" && row.folder) {
                  const skills = skillsInFolder(row.folder.id, bank);
                  const checked = allSelected(own, skills);
                  const some = !checked && skills.some((skill) => ownIds.has(skill.id));
                  return (
                    <li
                      key={`f-${row.folder.id}`}
                      className="ln-skills__row"
                      style={{ paddingLeft: 4 + row.depth * 12 }}
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
                        title="Checks its skills now"
                      />
                      {/* The chevron stays live in read-only: folding a folder
                          reads the tree, it changes no selection. */}
                      <button
                        type="button"
                        onClick={() => toggleExpanded(row.folder!.id)}
                        className="ln-skills__folder"
                        aria-expanded={row.expanded}
                      >
                        {row.expanded ? <ChevronDown size={12} strokeWidth={1.5} /> : <ChevronRight size={12} strokeWidth={1.5} />}
                        {row.expanded ? <FolderOpen size={12} strokeWidth={1.5} /> : <Folder size={12} strokeWidth={1.5} />}
                        <span className="ln-ellipsis">{row.folder.name}</span>
                        <span className="ln-faint ln-num">{row.count}</span>
                      </button>
                    </li>
                  );
                }
                if (row.ref.kind === "skill" && row.skill) {
                  const skill = row.skill;
                  const isOwn = ownIds.has(skill.id);
                  const from = inheritedById.get(skill.id);
                  const locked = readOnly || (!isOwn && !!from);
                  const inheritedTitle = from && !isOwn ? `Inherited from the ${from.map((t) => SKILL_TIER_LABEL[t].toLowerCase()).join(", ")} tier` : undefined;
                  return (
                    <li
                      key={`s-${skill.id}`}
                      className="ln-skills__row"
                      style={{ paddingLeft: 4 + row.depth * 12 }}
                      data-testid={`${testId}-option-${skill.id}`}
                    >
                      <input
                        type="checkbox"
                        aria-label={skill.name}
                        data-testid={`${testId}-check-${skill.id}`}
                        checked={isOwn || !!from}
                        disabled={locked}
                        onChange={() => toggleSkill(skill.id, skill.name)}
                        title={inheritedTitle}
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
                        className="ln-skills__name"
                        data-inherited={(from && !isOwn) || undefined}
                      >
                        {skill.name}
                      </button>
                      {from && !isOwn && (
                        <span className="d-chip ln-skills__tier" title={inheritedTitle}>
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
          {readOnly && <div className="d-pop__empty">Frozen: read-only here</div>}
        </div>
      </AnchoredPanel>
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
