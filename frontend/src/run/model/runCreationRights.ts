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
import { isAllowed, refusalOf } from "../../shared/authorization/api";
import { GENERIC_REFUSAL } from "../../shared/http/client";
import type { Project } from "../../workspace/model/project";


export interface RunCreationRights {
  /** The answer arrived. Before it, everything shows (the daemon decides). */
  known: boolean;
  /** The loupe: browsing the disk for a repository. */
  canBrowse: boolean;
  /** CORE-205: why the loupe is greyed (the refusal's sentence), else absent. */
  browseRefused?: string;
  /** A Run may be created in no Projet (a repository no Projet owns). */
  outsideProjects: boolean;
  /** The Projets a Run may be created in, in the order listed. */
  allowedProjects: Project[];
  /** The Projets listed where a Run may not be created. */
  hiddenProjects: Project[];
  /**
   * CORE-205: the sentence of the refusal of a Run in no Projet (else of the
   * first Projet refused), what a greyed « New Run » says when no Projet
   * allows a Run ({@link whyNoRunAnywhere}).
   */
  createRefused?: string;
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
    browseRefused: refusalOf(decisions[0]),
    outsideProjects: isAllowed(decisions[1]),
    allowedProjects,
    hiddenProjects,
    createRefused: refusalOf(decisions[1]) ?? projects.map((_, i) => refusalOf(decisions[i + 2])).find(Boolean),
  };
}

/** Something is refused: the form lists the allowed repositories by Projet. */
export function isRestricted(rights: RunCreationRights): boolean {
  return (
    rights.known &&
    (!rights.canBrowse || !rights.outsideProjects || rights.hiddenProjects.length > 0)
  );
}

/**
 * The repository field only searches the repositories of the caller's
 * Projets: it may not read the disk, so no path or URL of its own, and no Run
 * in no Projet (a repository of no Projet is read from the disk).
 */
export function searchesOnly(rights: RunCreationRights): boolean {
  return rights.known && !rights.canBrowse;
}

/** A Run may be created in no Projet: allowed there, and the disk may be read. */
export function canCreateOutsideProjects(rights: RunCreationRights): boolean {
  return rights.outsideProjects && rights.canBrowse;
}

/** A Run may be created somewhere (the « New Run » button is enabled). */
export function canCreateSomeRun(rights: RunCreationRights): boolean {
  return !rights.known || canCreateOutsideProjects(rights) || rights.allowedProjects.length > 0;
}

/**
 * Why « New Run » is greyed: no Projet allows a Run, nor the disk a Run in
 * none; the sentence the extension gave with the refusal, else the core's
 * generic one. `undefined`
 * when a Run may be created somewhere.
 */
export function whyNoRunAnywhere(rights: RunCreationRights): string | undefined {
  if (canCreateSomeRun(rights)) return undefined;
  // A Run in no Projet allowed but not the disk: the disk is what is missing.
  const disk = rights.outsideProjects ? rights.browseRefused : undefined;
  return disk ?? rights.createRefused ?? rights.browseRefused ?? GENERIC_REFUSAL;
}

/** A group of repositories the restricted form offers, under its Projet's name. */
export interface RepoGroup {
  label: string;
  repos: string[];
  /** The Projet of the group; none for « Other ». */
  projectId?: string;
}

/**
 * The repositories the restricted form offers: each allowed Projet's, under
 * its name (sorted by name), then — when a Run may be created in no Projet
 * ({@link canCreateOutsideProjects}) — the recent repositories no Projet
 * owns, under « Other ».
 */
export function repoGroups(rights: RunCreationRights, recentRepos: string[], projects: Project[]): RepoGroup[] {
  const groups: RepoGroup[] = [...rights.allowedProjects]
    .sort((a, b) => a.name.localeCompare(b.name))
    .filter((p) => p.members.length > 0)
    .map((p) => ({ label: p.name, repos: [...p.members], projectId: p.id }));
  if (canCreateOutsideProjects(rights)) {
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
 * be created. Search-only, a recent one or none, and not one several Projets
 * hold unless `lastProject` (the Projet the form opens on) does: the form
 * would open asking to pick its Project. `null` when there is none.
 */
export function prefillRepo(
  rights: RunCreationRights,
  recentRepos: string[],
  projects: Project[],
  lastProject: string | null = null,
): string | null {
  if (!isRestricted(rights)) return recentRepos[0] ?? null;
  const offered = repoGroups(rights, recentRepos, projects).flatMap((g) => g.repos);
  if (!searchesOnly(rights)) return recentRepos.find((repo) => offered.includes(repo)) ?? offered[0] ?? null;
  const opensOn = rights.allowedProjects.find((p) => p.id === lastProject);
  const unambiguous = (repo: string) =>
    opensOn?.members.includes(repo) || projects.filter((p) => p.members.includes(repo)).length <= 1;
  return recentRepos.find((repo) => offered.includes(repo) && unambiguous(repo)) ?? null;
}

/** « N Project(s) hidden… », or `null` when none is. */
export function hiddenProjectsNote(rights: RunCreationRights): string | null {
  const n = rights.hiddenProjects.length;
  return n > 0 ? `${n} Project(s) hidden: you can't create Runs there.` : null;
}
