/**
 * What Settings › Projects lets the caller change (CORE-96), asked with the
 * question of authorization (`POST /authorize`): create a Projet, browse the
 * disk and create a repository, and per Projet update it (its name, its
 * repositories, its default primary) or delete it. Without extension every
 * answer is « allow ». A Projet the caller may not update reads as read-only;
 * each refused gesture stays, greyed, with the refusal's sentence (CORE-205).
 * The daemon still refuses what the page would show.
 */

import type { Decision, OperationQuestion } from "../../shared/authorization/api";
import { isAllowed, refusalOf } from "../../shared/authorization/api";
import type { Project } from "./project";

export interface ProjectRights {
  /** The answer arrived. Before it, everything shows (the daemon decides). */
  known: boolean;
  canCreate: boolean;
  /** The explorer: browsing the disk for a repository. */
  canBrowse: boolean;
  /** A new empty repository. */
  canCreateRepository: boolean;
  /** The Projets the caller may not update. */
  readOnly: ReadonlySet<string>;
  /** The Projets the caller may not delete. */
  undeletable: ReadonlySet<string>;
  /** Why each refused gesture is refused: the sentence its greyed control says (CORE-205). */
  why: ProjectRefusals;
}

/** The sentence of each refusal of {@link ProjectRights}. */
export interface ProjectRefusals {
  create?: string;
  browse?: string;
  createRepository?: string;
  /** By Projet id. */
  update: ReadonlyMap<string, string>;
  /** By Projet id. */
  delete: ReadonlyMap<string, string>;
}

export const UNKNOWN_PROJECT_RIGHTS: ProjectRights = {
  known: false,
  canCreate: true,
  canBrowse: true,
  canCreateRepository: true,
  readOnly: new Set(),
  undeletable: new Set(),
  why: { update: new Map(), delete: new Map() },
};

/** The Operations to ask about, for `projects`. */
export function projectQuestions(projects: Project[]): OperationQuestion[] {
  return [
    { resource: "project", verb: "create" },
    { resource: "repository", verb: "read" },
    { resource: "repository", verb: "create" },
    ...projects.flatMap((p): OperationQuestion[] => [
      { resource: "project", verb: "update", target: p.id },
      { resource: "project", verb: "delete", target: p.id },
    ]),
  ];
}

/** The rights the `decisions` to {@link projectQuestions} grant. */
export function projectRights(projects: Project[], decisions: Decision[]): ProjectRights {
  const readOnly = new Set<string>();
  const undeletable = new Set<string>();
  const update = new Map<string, string>();
  const remove = new Map<string, string>();
  projects.forEach((project, i) => {
    const whyNotUpdate = refusalOf(decisions[3 + 2 * i]);
    const whyNotDelete = refusalOf(decisions[4 + 2 * i]);
    if (whyNotUpdate !== undefined) {
      readOnly.add(project.id);
      update.set(project.id, whyNotUpdate);
    }
    if (whyNotDelete !== undefined) {
      undeletable.add(project.id);
      remove.set(project.id, whyNotDelete);
    }
  });
  return {
    known: true,
    canCreate: isAllowed(decisions[0]),
    canBrowse: isAllowed(decisions[1]),
    canCreateRepository: isAllowed(decisions[2]),
    readOnly,
    undeletable,
    why: {
      create: refusalOf(decisions[0]),
      browse: refusalOf(decisions[1]),
      createRepository: refusalOf(decisions[2]),
      update,
      delete: remove,
    },
  };
}
