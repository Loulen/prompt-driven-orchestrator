import type { Project } from "../types";
import type { ProjectRef } from "./groupByRepo";

/**
 * Build a **verbatim** `path → ProjectRef | null` lookup from the Projets list
 * (#552). ADR-0033: paths are matched by exact string equality, never
 * canonicalised — two spellings of a path are two paths. Shared by the Runs and
 * Triggers panels so both group identically.
 */
export function projectLookup(
  projects: Project[],
): (path: string) => ProjectRef | null {
  const byPath = new Map<string, ProjectRef>();
  for (const p of projects) {
    for (const path of p.members) byPath.set(path, { id: p.id, name: p.name });
  }
  return (path) => byPath.get(path) ?? null;
}
