import type { Actor } from "../generated/Actor";
import type { ActorDisplay } from "../generated/ActorDisplay";
import { initials } from "./initials";
import { useActorDisplay } from "./store";

/** A round avatar: the extension's image, or the initials of the name. */
export function ActorAvatar({ display, size = 16 }: { display: ActorDisplay; size?: number }) {
  const style = { width: size, height: size, fontSize: Math.max(7, Math.round(size * 0.42)) };
  return display.avatar_url ? (
    <img
      src={display.avatar_url}
      alt=""
      className="shrink-0 rounded-full object-cover"
      style={style}
      data-testid="actor-avatar"
    />
  ) : (
    <span
      className="grid shrink-0 place-items-center rounded-full bg-acc font-semibold text-white"
      style={style}
      data-testid="actor-avatar"
    >
      {initials(display.name)}
    </span>
  );
}

/**
 * An Actor as the extensions name it: avatar and name. **Nothing** while the
 * name is unknown or when no extension names the Actor: the place keeps the
 * layout it has without extension.
 */
export default function ActorName({
  actor,
  className = "",
  size = 14,
}: {
  actor: Actor | null | undefined;
  className?: string;
  size?: number;
}) {
  const display = useActorDisplay(actor);
  if (!display) return null;
  return (
    <span
      className={`inline-flex min-w-0 items-center gap-1 ${className}`}
      title={display.name}
      data-testid="actor-name"
    >
      <ActorAvatar display={display} size={size} />
      <span className="truncate">{display.name}</span>
    </span>
  );
}
