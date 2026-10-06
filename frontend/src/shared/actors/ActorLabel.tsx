import type { Actor } from "../generated/Actor";
import { actorLabel } from "../lib/actor";
import { ActorAvatar } from "./ActorName";
import { useActorDisplay } from "./store";

/**
 * An Actor in text: its display name and avatar when an extension names it,
 * the core's own label otherwise (`alice (test)`, `Local operator`).
 */
export default function ActorLabel({ actor }: { actor: Actor }) {
  const display = useActorDisplay(actor);
  if (!display) return <>{actorLabel(actor)}</>;
  return (
    <span className="inline-flex items-center gap-1" title={actorLabel(actor)}>
      <ActorAvatar display={display} size={12} />
      {display.name}
    </span>
  );
}
