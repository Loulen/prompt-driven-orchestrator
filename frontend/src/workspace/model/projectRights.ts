/**
 * What Settings › Projects lets the caller change (CORE-96), asked with the
 * question of authorization (`POST /authorize`): create a Projet, browse the
 * disk, and per Projet update it (its name, its repositories, its default
 * primary), delete it, create a repository for it (a clone, a new empty one)
 * and read its repositories (to bring one into another Projet). Without
 * extension every answer is « allow ». A Projet the caller may not update
 * reads as read-only; each refused gesture stays, greyed, with the refusal's
 * sentence (CORE-205). The daemon still refuses what the page would show.
 */

import type { Decision, OperationQuestion } from "../../shared/authorization/api";
import { isAllowed, refusalOf } from "../../shared/authorization/api";
import type { Project } from "./project";

export interface ProjectRights {
  /** The answer arrived. Before it, everything shows (the daemon decides). */
  known: boolean;
  canCreate: boolean;
  /** The explorer: browsing the disk for a repository (a path in no Projet). */
  canBrowse: boolean;
  /** The Projets the caller may not update. */
  readOnly: ReadonlySet<string>;
  /** The Projets the caller may not delete. */
  undeletable: ReadonlySet<string>;
  /** The Projets whose repositories the caller may read, so bring into another of its Projets. */
  readsRepositoriesOf: ReadonlySet<string>;
  /** Why each refused gesture is refused: the sentence its greyed control says (CORE-205). */
  why: ProjectRefusals;
}

/** The sentence of each refusal of {@link ProjectRights}. */
export interface ProjectRefusals {
  create?: string;
  browse?: string;
  /** By Projet id. */
  update: ReadonlyMap<string, string>;
  /** By Projet id. */
  delete: ReadonlyMap<string, string>;
  /** Creating a repository for a Projet (clone, new empty), by Projet id. */
  createRepository: ReadonlyMap<string, string>;
}

export const UNKNOWN_PROJECT_RIGHTS: ProjectRights = {
  known: false,
  canCreate: true,
  canBrowse: true,
  readOnly: new Set(),
  undeletable: new Set(),
  readsRepositoriesOf: new Set(),
  why: { update: new Map(), delete: new Map(), createRepository: new Map() },
};

/** The questions asked once, then those asked per Projet. */
const GLOBAL = 2;
const PER_PROJECT = 4;

/** The Operations to ask about, for `projects`. */
export function projectQuestions(projects: Project[]): OperationQuestion[] {
  return [
    { resource: "project", verb: "create" },
    { resource: "repository", verb: "read" },
    ...projects.flatMap((p): OperationQuestion[] => [
      { resource: "project", verb: "update", target: p.id },
      { resource: "project", verb: "delete", target: p.id },
      { resource: "repository", verb: "create", projects: [p.id] },
      { resource: "repository", verb: "read", projects: [p.id] },
    ]),
  ];
}

/** The rights the `decisions` to {@link projectQuestions} grant. */
export function projectRights(projects: Project[], decisions: Decision[]): ProjectRights {
  const readOnly = new Set<string>();
  const undeletable = new Set<string>();
  const readsRepositoriesOf = new Set<string>();
  const update = new Map<string, string>();
  const remove = new Map<string, string>();
  const createRepository = new Map<string, string>();
  projects.forEach((project, i) => {
    const at = GLOBAL + PER_PROJECT * i;
    const whyNotUpdate = refusalOf(decisions[at]);
    const whyNotDelete = refusalOf(decisions[at + 1]);
    const whyNotCreateRepository = refusalOf(decisions[at + 2]);
    if (whyNotUpdate !== undefined) {
      readOnly.add(project.id);
      update.set(project.id, whyNotUpdate);
    }
    if (whyNotDelete !== undefined) {
      undeletable.add(project.id);
      remove.set(project.id, whyNotDelete);
    }
    if (whyNotCreateRepository !== undefined) createRepository.set(project.id, whyNotCreateRepository);
    if (isAllowed(decisions[at + 3])) readsRepositoriesOf.add(project.id);
  });
  return {
    known: true,
    canCreate: isAllowed(decisions[0]),
    canBrowse: isAllowed(decisions[1]),
    readOnly,
    undeletable,
    readsRepositoriesOf,
    why: {
      create: refusalOf(decisions[0]),
      browse: refusalOf(decisions[1]),
      update,
      delete: remove,
      createRepository,
    },
  };
}
