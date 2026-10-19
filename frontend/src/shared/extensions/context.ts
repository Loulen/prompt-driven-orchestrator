/**
 * The context every UI contribution receives (ADR-0081, amendment #29): the
 * selected Run and its Projet. App provides it (`ExtensionContextProvider`); each location adds what is at
 * stake there (`useContributions`).
 *
 * The Projet of a Run is the one the Runs list groups it under: the Projet whose
 * members hold its root's repository, compared verbatim (ADR-0033; a child Run
 * follows its root, #783).
 */

import { createContext, useContext } from "react";
import { rootOf } from "../../run/model/runTree";
import type { Project } from "../../workspace/model/project";
import type { RunListEntry } from "../generated/RunListEntry";
import type { RunState } from "../generated/RunState";
import type { Attributes } from "./attributes";

/** The selected Run, as a contribution sees it. */
export interface RunContext {
  id: string;
  name: string | null;
  pipeline: string;
  status: string;
  attributes: Attributes;
}

/** The Projet of the selected Run. */
export interface ProjectContext {
  id: string;
  name: string;
  attributes: Attributes;
}

/** What every location hands over, at least. */
export interface BaseContext {
  run: RunContext | null;
  project: ProjectContext | null;
}

export const NOTHING_SELECTED: BaseContext = { run: null, project: null };

/** Provided by `ExtensionContextProvider`. */
export const BaseContextValue = createContext<BaseContext>(NOTHING_SELECTED);

/** The Projet `runId` was launched in (its root's, as the Runs list groups it, ADR-0087); `null` without one. */
export function projectOfRun(
  runId: string,
  runs: RunListEntry[],
  projects: Project[],
): ProjectContext | null {
  const root = rootOf(runs, runId) ?? runs.find((r) => r.run_id === runId);
  if (!root) return null;
  const project = root.project_id ? projects.find((p) => p.id === root.project_id) : undefined;
  return project
    ? { id: project.id, name: project.name, attributes: project.attributes ?? {} }
    : null;
}

export function runContext(run: RunState): RunContext {
  return {
    id: run.run_id,
    name: run.name ?? null,
    pipeline: run.pipeline_name,
    status: run.status,
    attributes: run.attributes ?? {},
  };
}

/** The selected Run and its Projet; nothing outside a provider. */
export function useBaseContext(): BaseContext {
  return useContext(BaseContextValue);
}
