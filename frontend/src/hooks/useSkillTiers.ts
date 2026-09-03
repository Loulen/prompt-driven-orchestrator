import { useCallback, useEffect, useMemo, useState } from "react";
import { fetchProjects, fetchSettings } from "../api";
import type { Project, SkillRef } from "../types";
import type { InheritedTier } from "../lib/skillSelection";
import { SKILLS_CHANGED } from "./useSkillBank";

/**
 * Window bus fired after a tier's skills selection is saved (#669): instance
 * settings, a Projet. Selectors showing that tier as *inherited* refresh.
 */
export const SKILL_TIERS_CHANGED = "pdo:skill-tiers-changed";

export function announceSkillTiersChanged() {
  window.dispatchEvent(new Event(SKILL_TIERS_CHANGED));
}

/**
 * The coarser tiers a selector inherits from (#669, ADR-0062): the instance
 * selection, and the Projet owning `repoPath` (verbatim member match, ADR-0033)
 * when a repo is known. Read fresh on mount and on the two buses; a failed read
 * leaves the tier empty rather than blocking the selector.
 *
 * `instanceSkills`: a caller that already holds `GET /settings` (the run modal,
 * #452: one settings read per open) passes its list and no second read happens.
 */
export function useSkillTiers(
  repoPath?: string | null,
  enabled = true,
  instanceSkills?: SkillRef[],
) {
  const readsSettings = instanceSkills === undefined;
  const [fetchedInstance, setFetchedInstance] = useState<SkillRef[]>([]);
  const [projects, setProjects] = useState<Project[]>([]);

  const apply = useCallback(
    (
      settings: PromiseSettledResult<{ skills?: SkillRef[] } | null>,
      list: PromiseSettledResult<Project[]>,
    ) => {
      if (settings.status === "fulfilled" && settings.value) {
        setFetchedInstance(settings.value.skills ?? []);
      }
      if (list.status === "fulfilled" && Array.isArray(list.value)) setProjects(list.value);
    },
    [],
  );
  const read = useCallback(
    () =>
      Promise.allSettled([
        readsSettings ? fetchSettings() : Promise.resolve(null),
        fetchProjects(),
      ]),
    [readsSettings],
  );

  const refresh = useCallback(() => {
    if (!enabled) return;
    void read().then(([settings, list]) => apply(settings, list));
  }, [enabled, read, apply]);

  useEffect(() => {
    if (!enabled) return;
    let cancelled = false;
    // Initial load inline (a subscription-style callback, not a synchronous
    // setState in the effect body); the buses re-use `refresh`.
    void read().then(([settings, list]) => {
      if (!cancelled) apply(settings, list);
    });
    window.addEventListener(SKILL_TIERS_CHANGED, refresh);
    window.addEventListener(SKILLS_CHANGED, refresh);
    return () => {
      cancelled = true;
      window.removeEventListener(SKILL_TIERS_CHANGED, refresh);
      window.removeEventListener(SKILLS_CHANGED, refresh);
    };
  }, [enabled, read, apply, refresh]);

  const instance = instanceSkills ?? fetchedInstance;

  const project = useMemo(() => {
    const path = repoPath?.trim();
    if (!path) return null;
    return projects.find((candidate) => candidate.members.includes(path)) ?? null;
  }, [projects, repoPath]);

  const inherited = useMemo<InheritedTier[]>(() => {
    const tiers: InheritedTier[] = [{ tier: "instance", skills: instance }];
    if (project) tiers.push({ tier: "project", skills: project.skills ?? [], label: project.name });
    return tiers;
  }, [instance, project]);

  return { instance, project, inherited, refresh };
}
