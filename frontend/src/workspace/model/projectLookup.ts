import type { Project } from "./project";
import type { ProjectRef } from "../../lib/groupByRepo";
import type { DeletedProject } from "../../shared/generated/DeletedProject";

/**
 * Build a **verbatim** `path → ProjectRef | null` lookup from the Projets list
 * (#552): the first Projet holding the path (a path may be in several,
 * ADR-0087). ADR-0033: paths are matched by exact string equality, never
 * canonicalised — two spellings of a path are two paths. The pencil of a
 * group header reads it; the lists group by {@link recordedProject}.
 */
export function projectLookup(
  projects: Project[],
): (path: string) => ProjectRef | null {
  const byPath = new Map<string, ProjectRef>();
  for (const p of projects) {
    for (const path of p.members) {
      if (!byPath.has(path)) byPath.set(path, { id: p.id, name: p.name });
    }
  }
  return (path) => byPath.get(path) ?? null;
}

/**
 * The Projet recorded on a Run or a Trigger (ADR-0087), by id, as the lists
 * group it; `null` for none, or for a Projet the caller may not read. A Projet
 * since deleted stays the Run's (CORE-96): it is named after `deleted`, the
 * deleted Projets the daemon still names, and marked so.
 */
export function recordedProject(
  projects: Project[],
  deleted: DeletedProject[] = [],
): (projectId: string | null | undefined) => ProjectRef | null {
  const byId = new Map<string, ProjectRef>(deleted.map((p) => [p.id, deletedRef(p)]));
  for (const p of projects) byId.set(p.id, { id: p.id, name: p.name });
  return (projectId) => (projectId ? byId.get(projectId) ?? null : null);
}

/** A deleted Projet as the lists name it. */
function deletedRef(project: DeletedProject): ProjectRef {
  return { id: project.id, name: `${project.name} (deleted)`, deleted: true };
}
