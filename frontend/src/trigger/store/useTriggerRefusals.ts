/**
 * Which gestures on the Triggers the caller may not make, and why (CORE-205):
 * asked once per list (`POST /authorize`), so a refused gesture stays on
 * screen, greyed, its tooltip saying the refusal's sentence.
 *
 * The Operations are the daemon's: `trigger` × `update` (edit, enable,
 * disable), `delete`, `fire` (Run now) aimed at the Trigger and placed in its
 * Projet, as the daemon places them (a Trigger is in the Projet it launches
 * in, or in none); `pause` (the global pause and its resume) aims at none;
 * `create` is placed where the new Trigger would launch.
 */

import { useMemo } from "react";
import { useRefusals } from "../../shared/authorization/useRefusals";
import type { OperationQuestion, OperationVerb } from "../../shared/authorization/api";

/** What a Trigger's gestures need: its id and the Projet it launches in. */
export interface TriggerPlace {
  id: string;
  project_id?: string | null;
}

/** Why the caller may not make each gesture on one Trigger, absent when allowed. */
export interface TriggerRefusals {
  /** Edit, enable, disable. */
  update?: string;
  delete?: string;
  /** Run now. */
  fire?: string;
}

/** `verb` on the Trigger `t`, in its Projet (or in none), as the daemon asks it. */
export function triggerOperation(verb: OperationVerb, t: TriggerPlace): OperationQuestion {
  return { resource: "trigger", verb, target: t.id, projects: t.project_id ? [t.project_id] : [] };
}

const FIRE: OperationVerb = { action: "fire" };
const PAUSE: OperationQuestion = { resource: "trigger", verb: { action: "pause" }, projects: [] };
/** The guard dry-run: it runs a command, aimed at no Trigger. */
const TEST_GUARD: OperationQuestion = { resource: "trigger", verb: { action: "test_guard" }, projects: [] };

/** The three Operations of a Trigger's own gestures, in `TriggerRefusals` order. */
function gesturesOf(t: TriggerPlace): OperationQuestion[] {
  return [triggerOperation("update", t), triggerOperation("delete", t), triggerOperation(FIRE, t)];
}

const NONE: TriggerRefusals = {};

/**
 * The refusals of the Triggers list: per Trigger (`of`), the global pause, and
 * the creation, refused only when no place the caller can pick allows it (no
 * Projet, or one of `projectIds`). One question for the whole list.
 */
export function useTriggerListRefusals(
  triggers: readonly TriggerPlace[],
  projectIds: readonly string[] = [],
  active = true,
): { of: (id: string) => TriggerRefusals; pause?: string; create?: string } {
  const placesKey = JSON.stringify(triggers.map((t) => [t.id, t.project_id ?? null]));
  const projectsKey = JSON.stringify(projectIds);
  const operations = useMemo(() => {
    const places = JSON.parse(placesKey) as [string, string | null][];
    const projects = JSON.parse(projectsKey) as string[];
    return [
      PAUSE,
      ...[[], ...projects.map((p) => [p])].map((where): OperationQuestion => ({
        resource: "trigger",
        verb: "create",
        projects: where,
      })),
      ...places.flatMap(([id, project_id]) => gesturesOf({ id, project_id })),
    ];
  }, [placesKey, projectsKey]);
  const refusals = useRefusals(operations, active);
  return useMemo(() => {
    // The creation outside any Projet, then in each Projet.
    const creations = refusals.slice(1, 2 + (JSON.parse(projectsKey) as string[]).length);
    const create = creations.every((r) => r !== undefined) ? creations[0] : undefined;
    const rest = refusals.slice(1 + creations.length);
    const places = JSON.parse(placesKey) as [string, string | null][];
    const byId = new Map<string, TriggerRefusals>();
    places.forEach(([id], i) => {
      const [update, del, fire] = rest.slice(3 * i, 3 * i + 3);
      if (update || del || fire) byId.set(id, { update, delete: del, fire });
    });
    return { of: (id: string) => byId.get(id) ?? NONE, pause: refusals[0], create };
  }, [refusals, placesKey, projectsKey]);
}

/** The refusals of one Trigger's gestures, and of its guard's dry-run. */
export function useTriggerRefusals(
  trigger: TriggerPlace,
  active = true,
): TriggerRefusals & { testGuard?: string } {
  const { id, project_id } = trigger;
  const operations = useMemo(() => [...gesturesOf({ id, project_id }), TEST_GUARD], [id, project_id]);
  const [update, del, fire, testGuard] = useRefusals(operations, active);
  return useMemo(() => ({ update, delete: del, fire, testGuard }), [update, del, fire, testGuard]);
}
