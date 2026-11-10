import type { Actor } from "../generated/Actor";

/**
 * An Actor (API 0.8: the creator of a Run, the last editor of a Trigger), as
 * the interface names it. The core knows no person: an identified Actor shows
 * its id and the extension that identified it.
 */
export function actorLabel(actor: Actor): string {
  switch (actor.kind) {
    case "local_operator":
      return "Local operator";
    case "agent":
      return `Agent ${actor.node_id} of run ${actor.run_id.slice(-8)}`;
    case "trigger":
      return `Trigger ${actor.trigger_id}`;
    case "identified":
      return `${actor.id} (${actor.namespace})`;
  }
}

/** The identity of an Actor: one per person, whatever names it (the cache key of its display name, the hue of its avatar). */
export function actorKey(actor: Actor): string {
  switch (actor.kind) {
    case "local_operator":
      return "local_operator";
    case "agent":
      return `agent:${actor.run_id}/${actor.node_id}`;
    case "trigger":
      return `trigger:${actor.trigger_id}`;
    case "identified":
      return `identified:${actor.namespace}/${actor.id}`;
    default:
      return JSON.stringify(actor);
  }
}
