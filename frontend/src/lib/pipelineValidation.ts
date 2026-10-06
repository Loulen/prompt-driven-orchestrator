import type { PipelineListEntry, PipelineValidation, Project } from "../types";

/**
 * Pipelines validés par Projet (#974, CONTEXT.md « Pipeline validé ») — the pure
 * reading of a Pipeline's validation, shared by the launch forms (sections), the
 * Pipelines tab and the tab bar (indicator), and the Validate modal.
 *
 * A Pipeline with no validation on the wire is validated for all Projets: that is
 * what an absent row means on the daemon, and what keeps an older daemon's list
 * whole.
 */

const ALL: PipelineValidation = { kind: "all" };

export function validationOf(pipeline: Pick<PipelineListEntry, "validation"> | undefined): PipelineValidation {
  return pipeline?.validation ?? ALL;
}

/**
 * The Projet of a repository: the one whose members hold this exact path. Verbatim,
 * like every path comparison of the app (ADR-0033) — the daemon resolves a Run's
 * Projet the same way.
 */
export function projectOfRepo(projects: Project[], repoPath: string): Project | null {
  const path = repoPath.trim();
  if (!path) return null;
  return projects.find((p) => p.members.includes(path)) ?? null;
}

/**
 * The three sections of the Pipeline menu of New Run / Trigger, in the order shown:
 * the Projet's own validated Pipelines, then the ones validated for every Projet,
 * then — behind « Show test pipelines » — the ones validated for nobody.
 *
 * A Pipeline validated for OTHER Projets only is in none of them: it is not a test
 * Pipeline, and it is not offered here. A repository without a Projet gets an empty
 * first section. Each section keeps the list's order (the daemon's, by name).
 */
export interface PipelineSections {
  project: PipelineListEntry[];
  global: PipelineListEntry[];
  test: PipelineListEntry[];
}

export function pipelineSections(
  pipelines: PipelineListEntry[],
  projectId: string | null,
): PipelineSections {
  const sections: PipelineSections = { project: [], global: [], test: [] };
  for (const pipeline of pipelines) {
    const v = validationOf(pipeline);
    if (v.kind === "all") sections.global.push(pipeline);
    else if (v.kind === "none") sections.test.push(pipeline);
    else if (projectId != null && v.project_ids.includes(projectId)) sections.project.push(pipeline);
  }
  return sections;
}

/** Whether the launch form offers this Pipeline at all for this Projet (any section). */
export function isOffered(pipeline: PipelineListEntry, projectId: string | null): boolean {
  const v = validationOf(pipeline);
  return v.kind !== "projects" || (projectId != null && v.project_ids.includes(projectId));
}

/**
 * What the indicator shows: a globe and « All » for every Projet, otherwise the
 * number of Projets, and the names the tooltip lists. Ids of a Projet that no longer
 * exists are left out of both — the daemon drops them on the Projet's deletion, and
 * a list that is a beat stale must not count a ghost.
 */
export interface ValidationSummary {
  all: boolean;
  count: number;
  names: string[];
}

export function validationSummary(validation: PipelineValidation, projects: Project[]): ValidationSummary {
  if (validation.kind === "all") return { all: true, count: 0, names: [] };
  if (validation.kind === "none") return { all: false, count: 0, names: [] };
  const names = validation.project_ids
    .map((id) => projects.find((p) => p.id === id)?.name)
    .filter((n): n is string => n != null);
  return { all: false, count: names.length, names };
}

/** The tooltip of the indicator: who the Pipeline is offered to. */
export function validationTooltip(summary: ValidationSummary): string {
  if (summary.all) return "Validated for all projects (current and future)";
  if (summary.count === 0) return "Test pipeline: not validated for any project";
  return `Validated for: ${summary.names.join(", ")}`;
}

/**
 * The request the Validate modal sends for what is ticked. « All projects » wins over
 * any individual box; nothing ticked is « not validated ».
 */
export function validationFromChoice(all: boolean, projectIds: string[]): PipelineValidation {
  if (all) return ALL;
  if (projectIds.length === 0) return { kind: "none" };
  return { kind: "projects", project_ids: projectIds };
}
