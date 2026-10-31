import { Eye } from "lucide-react";
import type { Presence } from "../../shared/generated/Presence";
import { actorKey, useActorDisplays, useCurrentActor } from "../../shared/actors/store";
import { actorLabel } from "../../shared/lib/actor";
import { AvatarStack } from "../../shared/ui/kit";
import { useRunPresences } from "../store/presences";

/** The tooltip of an avatar: the name, and that the person has the Run open. */
const viewing = (name: string) => `${name} · Viewing now`;

/**
 * Who else has the Run open (API 0.12), in the canvas's corner: an eye and
 * the kit's stack of their avatars, three then « +n ». Nothing when nobody
 * else looks at it.
 */
export default function RunPresenceStack({ runId }: { runId: string }) {
  const presences = useRunPresences(runId);
  const me = useCurrentActor();
  const mine = me ? actorKey(me.actor) : null;
  const others = presences.filter((p) => actorKey(p.actor) !== mine);
  const displays = useActorDisplays(others.map((p) => p.actor));
  if (others.length === 0) return null;
  const person = (p: Presence, i: number) => {
    const display = displays[i];
    const name = display?.name ?? actorLabel(p.actor);
    return { key: actorKey(p.actor), name, actor: p.actor, src: display?.avatar_url, title: viewing(name) };
  };
  return (
    <span className="cv-pres" data-testid="run-presence" title={`Here now · ${others.length}`}>
      <Eye size={12} strokeWidth={1.5} aria-label="Here now" />
      <AvatarStack people={others.map(person)} size={20} />
    </span>
  );
}
