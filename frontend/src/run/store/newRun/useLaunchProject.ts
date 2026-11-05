import { useCallback, useEffect, useMemo, useRef } from "react";
import { prefillOf, type OpenIntent } from "../../model/openIntent";
import {
  initialProject,
  launchesInProject,
  offeredProjects,
  offersNoProject,
  projectSecondaries,
  readLastProject,
  writeLastProject,
} from "../../model/launchProject";
import { hiddenProjectsNote } from "../../model/runCreationRights";
import { useRunCreationRightsStore } from "../runCreationRightsStore";
import type { NewRunState } from "./useNewRunState";

/**
 * The Projet of New Run / Trigger (ADR-0087): which ones are offered, the one
 * the form opens on (the one an extension opened it with, if offered; else the
 * last one chosen in this browser, else the first one offered), and what
 * choosing one does — its default primary repository is
 * preselected, and the secondaries of the Projet left are dropped.
 */
export function useLaunchProject(open: boolean, openIntent: OpenIntent, s: NewRunState) {
  const rights = useRunCreationRightsStore((st) => st.rights);
  const rightsProjects = useRunCreationRightsStore((st) => st.projects);
  const {
    projects, projectId, setProjectId, projectChosen, setProjectChosen, targetRepo,
    handleRepoChange, setSecondaryRepos,
  } = s;
  const seeded = useRef(false);

  // The Projets of the form: those the daemon listed, as the Droits allow.
  const offered = useMemo(
    () => offeredProjects(rights, projects ?? rightsProjects),
    [rights, projects, rightsProjects],
  );
  const allowNone = offersNoProject(rights);
  const project = useMemo(
    () => (projectId ? (offered.find((p) => p.id === projectId) ?? null) : null),
    [offered, projectId],
  );

  // Switch to `next`: its default primary repository, and none of the
  // secondaries the Projet left offered.
  const apply = useCallback(
    (next: string | null, keepPrimary = false) => {
      setProjectId(next);
      setSecondaryRepos((prev) => prev.filter((r) => !r.fromProject));
      const primary = next ? offered.find((p) => p.id === next)?.primary_repo : undefined;
      if (primary && !keepPrimary) handleRepoChange(primary);
    },
    [offered, setProjectId, setSecondaryRepos, handleRepoChange],
  );

  const chooseProject = useCallback(
    (next: string | null) => {
      setProjectChosen(true);
      writeLastProject(next);
      apply(next);
    },
    [apply, setProjectChosen],
  );

  // The Projet the form opens on, once the Projets are known. A fresh open
  // only: an edited Trigger comes with its own (`useOpenIntentSeeding`). The
  // Droits answering later may take the default back, never a choice.
  useEffect(() => {
    if (!open) {
      seeded.current = false;
      return;
    }
    if (openIntent.kind === "edit-trigger" || projects === null) return;
    const stillOffered = projectId === null ? allowNone : offered.some((p) => p.id === projectId);
    if (seeded.current && (projectChosen || stillOffered)) return;
    seeded.current = true;
    // API 0.22: the Projet an extension opened the form with, if offered, else
    // the first offered one holding its primary repository; its repositories,
    // if any, stay the form's.
    const prefill = prefillOf(openIntent);
    const wanted = prefill?.project;
    const primary = prefill?.repositories?.[0];
    const initial =
      wanted && offered.some((p) => p.id === wanted)
        ? wanted
        : ((primary ? offered.find((p) => p.members.includes(primary))?.id : undefined) ??
          initialProject(offered, rights, readLastProject()));
    const keepPrimary = primary !== undefined;
    if (initial !== projectId || !stillOffered) apply(initial, keepPrimary);
    else if (initial) {
      // Reopened on the same Projet: its default primary still leads.
      const primary = offered.find((p) => p.id === initial)?.primary_repo;
      if (primary && !keepPrimary && !targetRepo.trim()) handleRepoChange(primary);
    }
  }, [open, openIntent, projects, offered, rights, allowNone, projectId, projectChosen, apply, targetRepo, handleRepoChange]);

  // A secondary that became the primary is no longer a secondary.
  useEffect(() => {
    const primary = targetRepo.trim();
    if (!primary) return;
    setSecondaryRepos((prev) =>
      prev.some((r) => r.path.trim() === primary)
        ? prev.filter((r) => r.path.trim() !== primary)
        : prev,
    );
  }, [targetRepo, setSecondaryRepos]);

  const otherRepos = useMemo(() => projectSecondaries(project, targetRepo), [project, targetRepo]);
  const primaryInProject = launchesInProject(project, targetRepo);

  return {
    offeredProjects: offered,
    allowNoProject: allowNone,
    launchProject: project,
    projectNote: hiddenProjectsNote(rights),
    chooseProject,
    projectRepos: otherRepos,
    primaryInProject,
  };
}
