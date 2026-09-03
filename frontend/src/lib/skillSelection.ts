import type { Skill, SkillBank, SkillRef, SkillTier } from "../types";
import { descendantFolderIds } from "./skillTree";

/**
 * Pure helpers behind the shared skills selector (#669, ADR-0062, CONTEXT.md
 * §*Banque de skills*). The frontend mirror of `skill_selection.rs`: the same
 * strict additive union of the four tiers, coarsest first, de-duplicated by id,
 * each row attributed to every tier that selected it. Identity is the id — the
 * bank's current name wins over the stored label, and an id the bank no longer
 * knows is a *warning* row, never a refusal.
 */

export const SKILL_TIER_ORDER: readonly SkillTier[] = ["instance", "project", "run", "node"];

export const SKILL_TIER_LABEL: Record<SkillTier, string> = {
  instance: "Instance",
  project: "Project",
  run: "Run",
  node: "Node",
};

/** One inherited tier as a selector receives it: which tier, and what it selected. */
export interface InheritedTier {
  tier: SkillTier;
  skills: SkillRef[];
  /** Optional origin label (a Projet's name, a Run's name) shown beside the tier. */
  label?: string;
}

/** One row of the effective list a selector renders. */
export interface EffectiveRow {
  id: string;
  /** The bank's current name, else the stored label. */
  name: string;
  /** Every tier that selected it, coarsest first. */
  tiers: SkillTier[];
  /** `true` when `ownTier` is among `tiers` — the checkbox is live. */
  own: boolean;
  /** `true` when at least one *other* tier selected it — shown greyed with its origin. */
  inherited: boolean;
  /** `true` when the bank no longer has the id — the warning row. */
  missing: boolean;
}

export interface EffectiveSkills {
  rows: EffectiveRow[];
  /** Distinct known skills the NodeRun would receive (missing ones excluded). */
  effectiveCount: number;
  missing: EffectiveRow[];
}

/**
 * Union `inherited` (coarser tiers) with `own` at `ownTier`, naming each id from
 * `bank`. Order: first tier that names an id decides its position.
 */
export function resolveEffectiveSkills(
  ownTier: SkillTier,
  own: SkillRef[],
  inherited: InheritedTier[],
  bank: Pick<SkillBank, "skills">,
): EffectiveSkills {
  const known = new Map(bank.skills.map((skill) => [skill.id, skill]));
  const order: string[] = [];
  const acc = new Map<string, { label: string; tiers: SkillTier[] }>();
  const slots: { tier: SkillTier; skills: SkillRef[] }[] = [
    ...inherited.map((entry) => ({ tier: entry.tier, skills: entry.skills })),
    { tier: ownTier, skills: own },
  ].sort((a, b) => SKILL_TIER_ORDER.indexOf(a.tier) - SKILL_TIER_ORDER.indexOf(b.tier));
  for (const { tier, skills } of slots) {
    for (const skill of skills) {
      const id = skill.id.trim();
      if (!id) continue;
      const entry = acc.get(id);
      if (entry) {
        if (!entry.tiers.includes(tier)) entry.tiers.push(tier);
      } else {
        order.push(id);
        acc.set(id, { label: skill.name, tiers: [tier] });
      }
    }
  }
  const rows: EffectiveRow[] = order.map((id) => {
    const { label, tiers } = acc.get(id)!;
    const inBank = known.get(id);
    const own = tiers.includes(ownTier);
    return {
      id,
      name: inBank?.name ?? label ?? id,
      tiers,
      own,
      inherited: tiers.some((tier) => tier !== ownTier),
      missing: !inBank,
    };
  });
  return {
    rows,
    effectiveCount: rows.filter((row) => !row.missing).length,
    missing: rows.filter((row) => row.missing),
  };
}

/** `{id, name}` references for `skills`, as a tier stores them. */
export function toRefs(skills: Skill[]): SkillRef[] {
  return skills.map((skill) => ({ id: skill.id, name: skill.name }));
}

/**
 * The gesture behind "check a folder" (ADR-0062 « Dossier = geste, pas
 * référence »): the skills in `folderId` and every folder below it, **at this
 * instant**. The caller adds them to its own list; nothing stores the folder.
 */
export function skillsInFolder(folderId: string, bank: SkillBank): Skill[] {
  const folders = descendantFolderIds(folderId, bank.folders);
  return bank.skills.filter((skill) => skill.folder_id != null && folders.has(skill.folder_id));
}

/** Add `refs` to `own`, keeping order and de-duplicating by id. */
export function addRefs(own: SkillRef[], refs: SkillRef[]): SkillRef[] {
  const seen = new Set(own.map((skill) => skill.id));
  const out = [...own];
  for (const ref of refs) {
    if (!seen.has(ref.id)) {
      seen.add(ref.id);
      out.push(ref);
    }
  }
  return out;
}

export function removeRef(own: SkillRef[], id: string): SkillRef[] {
  return own.filter((skill) => skill.id !== id);
}

/** Are *all* of `skills` in `own`? (empty ⇒ false: nothing to check) */
export function allSelected(own: SkillRef[], skills: Skill[]): boolean {
  if (skills.length === 0) return false;
  const ids = new Set(own.map((skill) => skill.id));
  return skills.every((skill) => ids.has(skill.id));
}

/** Ids `own` selects that the bank no longer has. */
export function missingIds(own: SkillRef[], bank: Pick<SkillBank, "skills">): SkillRef[] {
  const known = new Set(bank.skills.map((skill) => skill.id));
  return own.filter((skill) => !known.has(skill.id));
}

/** `3 effective skills` / `1 effective skill` / `No skill`. */
export function effectiveCountLabel(count: number): string {
  if (count === 0) return "No skill";
  return `${count} effective skill${count === 1 ? "" : "s"}`;
}
