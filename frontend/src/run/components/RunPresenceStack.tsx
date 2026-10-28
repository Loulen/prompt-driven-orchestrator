import { Eye } from "lucide-react";
import type { Presence } from "../../shared/generated/Presence";
import { actorKey, useActorDisplay, useCurrentActor } from "../../shared/actors/store";
import { actorLabel } from "../../shared/lib/actor";
import { Avatar } from "../../shared/ui/kit";
import { useRunPresences } from "../store/presences";

/** How many avatars the stack shows before a count. */
const MAX = 3;

function PresenceAvatar({ presence }: { presence: Presence }) {
  const display = useActorDisplay(presence.actor);
  return <Avatar name={display?.name ?? actorLabel(presence.actor)} src={display?.avatar_url} size={16} />;
}

/**
 * Who else has the Run open (API 0.12), in the canvas's corner: an eye and
 * the stack of their initials, past three a count. Nothing when nobody else
 * looks at it.
 */
export default function RunPresenceStack({ runId }: { runId: string }) {
  const presences = useRunPresences(runId);
  const me = useCurrentActor();
  const mine = me ? actorKey(me.actor) : null;
  const others = presences.filter((p) => actorKey(p.actor) !== mine);
  if (others.length === 0) return null;
  return (
    <span className="cv-pres" data-testid="run-presence" title={`Here now · ${others.length}`}>
      <Eye size={12} strokeWidth={1.5} aria-label="Here now" />
      <span className="cv-stack">
        {others.slice(0, MAX).map((p) => (
          <PresenceAvatar key={actorKey(p.actor)} presence={p} />
        ))}
        {others.length > MAX && <span className="cv-stack__more">+{others.length - MAX}</span>}
      </span>
    </span>
  );
}
