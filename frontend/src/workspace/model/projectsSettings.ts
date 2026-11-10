/**
 * Settings › Projects (CORE-96): the rules of the page, apart from React. The
 * Projets listed and counted, the other Projets holding a repository (ADR-0087),
 * and the folder a new empty repository is created in.
 */

import type { Project } from "./project";

/**
 * Where Settings › Projects opens: on a Projet, or on a new one to name, with
 * the repositories it starts with (the pencil of a group in no Projet).
 */
export type ProjectsEntry =
  | { projectId: string }
  | { draft: { name: string; members: string[] } };

/** The Projets as the page lists them: by name, then by id. */
export function sortedProjects(projects: Project[]): Project[] {
  return [...projects].sort(
    (a, b) => a.name.localeCompare(b.name) || (a.id < b.id ? -1 : a.id > b.id ? 1 : 0),
  );
}

/** How many of `runs` were launched in each Projet (ADR-0087: the one recorded). */
export function runCountByProject(
  runs: ReadonlyArray<{ project_id?: string | null }>,
): Map<string, number> {
  const counts = new Map<string, number>();
  for (const run of runs) {
    if (run.project_id) counts.set(run.project_id, (counts.get(run.project_id) ?? 0) + 1);
  }
  return counts;
}

/** The names of the Projets other than `projectId` holding `path` (ADR-0087). */
export function alsoIn(projects: Project[], projectId: string, path: string): string[] {
  return sortedProjects(projects)
    .filter((p) => p.id !== projectId && p.members.includes(path))
    .map((p) => p.name);
}

/** `count` repositories, as the list shows it. */
export function repoCount(count: number): string {
  return `${count} ${count === 1 ? "repo" : "repos"}`;
}

/** `count` Runs, as the detail shows it. */
export function runCount(count: number): string {
  return `${count} ${count === 1 ? "run" : "runs"}`;
}

/** The recent repositories `project` does not hold yet, in their order. */
export function recentToAdd(recent: string[], project: Project): string[] {
  return recent.filter((path) => !project.members.includes(path));
}

/**
 * The folder typed for a new empty repository, cut into the parent and the
 * name `POST /repos/create` reads. `~` stands for `home`. `null` while the
 * path is not an absolute path with a folder name at its end.
 */
export function newRepositoryTarget(
  typed: string,
  home: string | null,
): { parent: string; name: string } | null {
  let path = typed.trim();
  if (path === "~" || path.startsWith("~/")) {
    if (!home) return null;
    path = home.replace(/\/+$/, "") + path.slice(1);
  }
  path = path.replace(/\/+$/, "");
  if (!path.startsWith("/")) return null;
  const cut = path.lastIndexOf("/");
  const name = path.slice(cut + 1);
  if (!name || name === "." || name === "..") return null;
  return { parent: cut === 0 ? "/" : path.slice(0, cut), name };
}

/**
 * Where the pencil of a list's group header opens Settings › Projects: on its
 * Projet, or, for a group in no Projet, on a new one named after the group
 * and holding its repository.
 */
export function projectsEntryOf(group: {
  kind: "project" | "path";
  key: string;
  repoPath: string;
  label: string;
}): ProjectsEntry {
  if (group.kind === "project") return { projectId: group.key.slice("project:".length) };
  return { draft: { name: group.label, members: group.repoPath ? [group.repoPath] : [] } };
}
