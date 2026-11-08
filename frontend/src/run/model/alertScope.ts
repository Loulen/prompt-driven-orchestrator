/**
 * The **Portée d'alerte** (CONTEXT.md) and the switches of the Alerte d'attente:
 * this browser's preferences, and which Runs may alert.
 *
 * The Portée only **restricts** what the live stream brings, which already
 * carries only the Runs the Actor's Droits let through:
 * - `launched` (default): the Runs whose root the current Actor created, so a
 *   Run an orchestration I started has created alerts me too;
 * - `projects`: the Runs whose root was launched in one of the checked Projets;
 * - `all`: every Run the stream brings.
 *
 * Pure: the root is resolved through what the caller knows (the Runs list) and
 * what it may ask the daemon (`GET /runs/{id}`), both injected.
 */
import type { Actor } from "../../shared/generated/Actor";
import { actorKey } from "../../shared/lib/actor";

export type AlertScope = "launched" | "projects" | "all";

export const ALERT_SCOPES: readonly AlertScope[] = ["launched", "projects", "all"];

/** This browser's preferences (Settings › Notifications). */
export interface WaitingAlertPrefs {
  /** The browser notification: off by default. */
  notification: boolean;
  /** The chime: off by default. */
  sound: boolean;
  scope: AlertScope;
  /** The Projets checked for the `projects` scope. */
  projects: string[];
}

export const DEFAULT_WAITING_ALERT_PREFS: WaitingAlertPrefs = {
  notification: false,
  sound: false,
  scope: "launched",
  projects: [],
};

/** A stored value read back: anything unknown falls to its default. */
export function normalizePrefs(value: unknown): WaitingAlertPrefs {
  if (!value || typeof value !== "object") return { ...DEFAULT_WAITING_ALERT_PREFS };
  const record = value as Record<string, unknown>;
  const scope = ALERT_SCOPES.find((s) => s === record.scope) ?? DEFAULT_WAITING_ALERT_PREFS.scope;
  const projects = Array.isArray(record.projects)
    ? [...new Set(record.projects.filter((id): id is string => typeof id === "string" && id !== ""))]
    : [];
  return { notification: record.notification === true, sound: record.sound === true, scope, projects };
}

/** What the Portée reads of a Run: its provenance. */
export interface RunLineage {
  run_id: string;
  parent_run_id?: string;
  /** The top of its chain of parents, when the list says it. */
  root_run_id?: string;
  created_by?: Actor;
  project_id?: string;
}

/** How deep a chain of parents is climbed without the list's `root_run_id`. */
const MAX_DEPTH = 32;

/**
 * The root of `runId`: the Run that launched its chain. Read off the list
 * first, else asked; `null` when the Run itself cannot be read. A parent that
 * cannot be read ends the climb: the last Run read stands for the root.
 */
export async function resolveRoot(
  runId: string,
  known: (id: string) => RunLineage | undefined,
  fetch: (id: string) => Promise<RunLineage | null>,
): Promise<RunLineage | null> {
  const read = async (id: string) => known(id) ?? (await fetch(id).catch(() => null));
  let run = await read(runId);
  if (!run) return null;
  if (run.root_run_id && run.root_run_id !== run.run_id) {
    const root = await read(run.root_run_id);
    if (root) return root;
  }
  const seen = new Set([run.run_id]);
  for (let depth = 0; run.parent_run_id && depth < MAX_DEPTH; depth += 1) {
    if (seen.has(run.parent_run_id)) break;
    const parent = await read(run.parent_run_id);
    if (!parent) break;
    seen.add(parent.run_id);
    run = parent;
  }
  return run;
}

/** Does the Portée let the Run whose root is `root` alert? */
export function scopeAllows(
  prefs: Pick<WaitingAlertPrefs, "scope" | "projects">,
  root: RunLineage | null,
  me: Actor | null,
): boolean {
  switch (prefs.scope) {
    case "all":
      return true;
    case "launched":
      return !!root?.created_by && !!me && actorKey(root.created_by) === actorKey(me);
    case "projects":
      return !!root?.project_id && prefs.projects.includes(root.project_id);
  }
}
