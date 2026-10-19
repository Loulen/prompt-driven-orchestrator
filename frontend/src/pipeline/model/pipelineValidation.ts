import type { PipelineListEntry } from "../../shared/generated/PipelineListEntry";
import type { PipelineValidation } from "../../shared/generated/PipelineValidation";
import type { Project } from "../../workspace/model/project";

/**
 * Pipelines validés par Projet (CONTEXT.md « Pipeline validé ») — the pure
 * reading of a Pipeline's validation, shared by the launch forms (sections), the
 * Pipelines tab and the tab bar (indicator), and the Validate modal.
 *
 * A Pipeline with no validation on the wire is validated for all Projets: that is
 * what an absent row means on the daemon. One validated for all may be listed first
 * for some Projets (`project_ids` of `all`, absent = none).
 */

const ALL: PipelineValidation = { kind: "all" };

export function validationOf(
  pipeline: Pick<PipelineListEntry, "validation"> | undefined,
): PipelineValidation {
  return pipeline?.validation ?? ALL;
}

/** The Projets it is offered to, or listed first for when validated for all. */
export function projectIdsOf(validation: PipelineValidation): string[] {
  return validation.kind === "none" ? [] : (validation.project_ids ?? []);
}

/**
 * The Projet of a repository: the one whose members hold this exact path. Verbatim,
 * like every path comparison of the app (ADR-0033).
 */
export function projectOfRepo(projects: Project[], repoPath: string): Project | null {
  const path = repoPath.trim();
  if (!path) return null;
  return projects.find((p) => p.members.includes(path)) ?? null;
}

/**
 * The three sections of the Pipeline menu of New Run / Trigger, in the order shown:
 * the Projet's own Pipelines (validated for it, or validated for all and listed first
 * for it), then the other ones validated for every Projet, then — behind « Show test
 * pipelines » — the ones validated for nobody.
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
    const forProject = projectId != null && projectIdsOf(v).includes(projectId);
    if (v.kind === "none") sections.test.push(pipeline);
    else if (forProject) sections.project.push(pipeline);
    else if (v.kind === "all") sections.global.push(pipeline);
  }
  return sections;
}

/** Whether the launch form offers this Pipeline at all for this Projet (any section). */
export function isOffered(pipeline: PipelineListEntry, projectId: string | null): boolean {
  const v = validationOf(pipeline);
  return v.kind !== "projects" || (projectId != null && v.project_ids.includes(projectId));
}

/**
 * The Pipeline the form picks for you: the Projet's first, else the first global
 * one. A test Pipeline is never chosen for you.
 */
export function defaultPipeline(sections: PipelineSections): PipelineListEntry | undefined {
  return sections.project[0] ?? sections.global[0];
}

/**
 * What the indicator shows: a globe and « All » for every Projet, otherwise the
 * number of Projets, and the names the tooltip lists (for « All », the Projets it
 * is listed first for). Ids of a Projet the actor
 * cannot read (or that no longer exists) are left out of both: a list that is a
 * beat stale, or filtered by Projet, must not count a ghost.
 */
export interface ValidationSummary {
  all: boolean;
  count: number;
  names: string[];
}

export function validationSummary(
  validation: PipelineValidation,
  projects: Project[],
): ValidationSummary {
  const names = projectIdsOf(validation)
    .map((id) => projects.find((p) => p.id === id)?.name)
    .filter((n): n is string => n != null);
  if (validation.kind === "all") return { all: true, count: 0, names };
  return { all: false, count: names.length, names };
}

/** The tooltip of the indicator: who the Pipeline is offered to. */
export function validationTooltip(summary: ValidationSummary): string {
  if (summary.all) {
    const all = "Validated for all projects (current and future)";
    return summary.names.length > 0 ? `${all}, listed first for: ${summary.names.join(", ")}` : all;
  }
  if (summary.count === 0) return "Test pipeline: not validated for any project";
  return `Validated for: ${summary.names.join(", ")}`;
}

/**
 * The request the Validate modal sends for what is ticked. Under « All projects » the
 * boxes are the Projets it is listed first for; otherwise nothing ticked is « not
 * validated ».
 */
export function validationFromChoice(all: boolean, projectIds: string[]): PipelineValidation {
  if (all) return projectIds.length > 0 ? { kind: "all", project_ids: projectIds } : ALL;
  if (projectIds.length === 0) return { kind: "none" };
  return { kind: "projects", project_ids: projectIds };
}
