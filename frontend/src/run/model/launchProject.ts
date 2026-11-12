/**
 * The Projet a New Run / Trigger launches in (ADR-0087): the form starts with
 * it, preselects its default primary repository and offers its other
 * repositories as secondaries. Pure: the form holds the state, these decide.
 */

import type { Project } from "../../workspace/model/project";
import { canCreateOutsideProjects, type RepoGroup, type RunCreationRights } from "./runCreationRights";

/** Where the last Projet chosen in New Run is remembered (this browser). */
export const LAST_PROJECT_KEY = "pdo.newRun.lastProject";
/** The remembered value of « No project ». */
export const NO_PROJECT = "";

/**
 * The Projets the form offers, by name: every Projet listed, or — when
 * something is refused (CORE-5) — those where a Run may be created.
 */
export function offeredProjects(rights: RunCreationRights, projects: Project[]): Project[] {
  const offered = rights.known && rights.hiddenProjects.length > 0 ? rights.allowedProjects : projects;
  return [...offered].sort((a, b) => a.name.localeCompare(b.name));
}

/** Whether « No project » is offered: a Run may be created in no Projet. */
export function offersNoProject(rights: RunCreationRights): boolean {
  return canCreateOutsideProjects(rights);
}

/**
 * The Projet the form opens on: the one chosen last time while it is still
 * offered (« No project » included, when allowed), else the first one offered,
 * else none. `searchOnly`, none rather than the first one: the repository
 * picked will set it.
 */
export function initialProject(
  offered: Project[],
  rights: RunCreationRights,
  last: string | null,
  searchOnly = false,
): string | null {
  if (last !== null) {
    if (last === NO_PROJECT && offersNoProject(rights)) return null;
    if (offered.some((p) => p.id === last)) return last;
  }
  return searchOnly ? null : (offered[0]?.id ?? null);
}

/** The repositories of `project` the form offers as secondaries: every member but the primary. */
export function projectSecondaries(project: Project | null, primary: string): string[] {
  if (!project) return [];
  const chosen = primary.trim();
  return project.members.filter((repo) => repo !== chosen);
}

/** Whether `repo` is a member of `project` (verbatim, ADR-0033, or without its trailing `/` or `/.`). */
function holds(project: Project, repo: string): boolean {
  const path = repo.trim().replace(/\/+$/, "").replace(/\/\.$/, "");
  return project.members.some((member) => member === repo.trim() || member === path);
}

/** Whether `repo` is a member of `project` (verbatim, ADR-0033), or no Projet is chosen. */
export function launchesInProject(project: Project | null, repo: string): boolean {
  return !project || holds(project, repo);
}

/** The Projets of `projects` holding `repo`, in their order. */
export function projectsHolding(projects: Project[], repo: string): Project[] {
  return repo.trim() === "" ? [] : projects.filter((project) => holds(project, repo));
}

/** « Alpha », « Alpha and Beta », « Alpha, Beta and Gamma ». */
function andList(names: string[]): string {
  return names.length <= 1 ? (names[0] ?? "") : `${names.slice(0, -1).join(", ")} and ${names[names.length - 1]}`;
}

/** By name, case aside, then as written: the daemon's order, for the same sentence. */
function byName(a: string, b: string): number {
  const [la, lb] = [a.toLowerCase(), b.toLowerCase()];
  if (la !== lb) return la < lb ? -1 : 1;
  return a < b ? -1 : a > b ? 1 : 0;
}

/**
 * A launch in no Projet on a repository several Projets hold: the daemon
 * refuses it with this sentence (the CLI says the same), the form before.
 * The Projets are named by name, in the daemon's order.
 */
export function ambiguousRepository(names: string[]): string {
  return `This repository is in ${andList([...names].sort(byName))}: pick its Project.`;
}

/**
 * The repositories the search-only field offers: those of the chosen Projet,
 * else of every Projet offered, each under its Projet (a repository of two
 * Projets under both).
 */
export function projectRepoGroups(offered: Project[], chosen: Project | null): RepoGroup[] {
  return (chosen ? [chosen] : offered)
    .filter((p) => p.members.length > 0)
    .map((p) => ({ label: p.name, repos: [...p.members], projectId: p.id }));
}

/** « 3 repos », « 1 repo ». */
export function repoCount(project: Project): string {
  const n = project.members.length;
  return `${n} repo${n === 1 ? "" : "s"}`;
}

/**
 * The folder names of a Projet's repositories, as the menu's second line shows
 * them: its default primary repository first, the others in their order.
 */
export function memberNames(project: Project): string {
  const primary = project.primary_repo;
  const members = primary && project.members.includes(primary)
    ? [primary, ...project.members.filter((repo) => repo !== primary)]
    : project.members;
  return members
    .map((repo) => repo.replace(/\/+$/, "").split("/").pop() || repo)
    .join(" · ");
}

/** The last Projet chosen in this browser; `null` when none was. */
export function readLastProject(): string | null {
  try {
    return window.localStorage.getItem(LAST_PROJECT_KEY);
  } catch {
    return null;
  }
}

/** Remember `projectId` (`null`: « No project ») for the next New Run. */
export function writeLastProject(projectId: string | null): void {
  try {
    window.localStorage.setItem(LAST_PROJECT_KEY, projectId ?? NO_PROJECT);
  } catch {
    // a browser refusing storage just forgets
  }
}
