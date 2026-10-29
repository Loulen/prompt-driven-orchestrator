/**
 * Where the caller may create a Run (CORE-5), as the New Run form and the
 * « New Run » button read it: whether the loupe (browsing the disk) is
 * allowed, whether a Run may be created in no Projet, and in which Projets.
 *
 * Asked with the question of authorization (`POST /authorize`): one
 * Operation for the disk, one for a Run in no Projet, one per Projet listed.
 * Without extension every answer is « allow », and the form is unchanged.
 */

import type { Decision, OperationQuestion } from "../../shared/authorization/api";
import { isAllowed } from "../../shared/authorization/api";
import type { Project } from "../../workspace/model/project";

/** Why New run is disabled (CORE-5). */
export const NO_RUN_ANYWHERE = "You can't create Runs in any Project. Ask an administrator for access.";

export interface RunCreationRights {
  /** The answer arrived. Before it, everything shows (the daemon decides). */
  known: boolean;
  /** The loupe: browsing the disk for a repository. */
  canBrowse: boolean;
  /** A Run may be created in no Projet (a repository no Projet owns). */
  outsideProjects: boolean;
  /** The Projets a Run may be created in, in the order listed. */
  allowedProjects: Project[];
  /** The Projets listed where a Run may not be created. */
  hiddenProjects: Project[];
}

/** Nothing known yet: everything shows, as without extension. */
export const UNKNOWN_RIGHTS: RunCreationRights = {
  known: false,
  canBrowse: true,
  outsideProjects: true,
  allowedProjects: [],
  hiddenProjects: [],
};

/** The Operations to ask about, for `projects`. */
export function runCreationQuestions(projects: Project[]): OperationQuestion[] {
  return [
    { resource: "repository", verb: "read" },
    { resource: "run", verb: "create", projects: [] },
    ...projects.map((p) => ({ resource: "run", verb: "create" as const, projects: [p.id] })),
  ];
}

/** The rights the `decisions` to {@link runCreationQuestions} grant. */
export function runCreationRights(projects: Project[], decisions: Decision[]): RunCreationRights {
  const allowedProjects: Project[] = [];
  const hiddenProjects: Project[] = [];
  projects.forEach((project, i) => {
    (isAllowed(decisions[i + 2]) ? allowedProjects : hiddenProjects).push(project);
  });
  return {
    known: true,
    canBrowse: isAllowed(decisions[0]),
    outsideProjects: isAllowed(decisions[1]),
    allowedProjects,
    hiddenProjects,
  };
}

/** Something is refused: the form lists the allowed repositories by Projet. */
export function isRestricted(rights: RunCreationRights): boolean {
  return (
    rights.known &&
    (!rights.canBrowse || !rights.outsideProjects || rights.hiddenProjects.length > 0)
  );
}

/** A Run may be created somewhere (the « New Run » button is enabled). */
export function canCreateSomeRun(rights: RunCreationRights): boolean {
  return !rights.known || rights.outsideProjects || rights.allowedProjects.length > 0;
}

/** A group of repositories the restricted form offers, under its Projet's name. */
export interface RepoGroup {
  label: string;
  repos: string[];
}

/**
 * The repositories the restricted form offers: each allowed Projet's, under
 * its name (sorted by name), then — when a Run may be created in no Projet —
 * the recent repositories no Projet owns, under « Other ».
 */
export function repoGroups(rights: RunCreationRights, recentRepos: string[], projects: Project[]): RepoGroup[] {
  const groups: RepoGroup[] = [...rights.allowedProjects]
    .sort((a, b) => a.name.localeCompare(b.name))
    .filter((p) => p.members.length > 0)
    .map((p) => ({ label: p.name, repos: [...p.members] }));
  if (rights.outsideProjects) {
    const owned = new Set(projects.flatMap((p) => p.members));
    const others = recentRepos.filter((repo) => !owned.has(repo));
    if (others.length > 0) groups.push({ label: "Other", repos: others });
  }
  return groups;
}

/**
 * The repository the form opens on. Unrestricted, the most recent one.
 * Restricted, the most recent one the form offers ({@link repoGroups}), else
 * the first one it offers: never a repository of a Projet where a Run may not
 * be created. `null` when there is none.
 */
export function prefillRepo(rights: RunCreationRights, recentRepos: string[], projects: Project[]): string | null {
  if (!isRestricted(rights)) return recentRepos[0] ?? null;
  const offered = repoGroups(rights, recentRepos, projects).flatMap((g) => g.repos);
  return recentRepos.find((repo) => offered.includes(repo)) ?? offered[0] ?? null;
}

/** « N Project(s) hidden… », or `null` when none is. */
export function hiddenProjectsNote(rights: RunCreationRights): string | null {
  const n = rights.hiddenProjects.length;
  return n > 0 ? `${n} Project(s) hidden: you can't create Runs there.` : null;
}
