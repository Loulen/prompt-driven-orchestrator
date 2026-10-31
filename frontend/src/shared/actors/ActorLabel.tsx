import type { Actor } from "../generated/Actor";
import { actorLabel } from "../lib/actor";
import { Avatar } from "../ui/kit";
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
      <Avatar name={display.name} actor={actor} src={display.avatar_url} size={16} />
      {display.name}
    </span>
  );
}
