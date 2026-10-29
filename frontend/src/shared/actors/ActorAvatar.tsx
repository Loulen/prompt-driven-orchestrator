import type { ActorDisplay } from "../generated/ActorDisplay";
import { initials } from "./initials";

/** A round avatar: the extension's image, or the initials of the name. */
export function ActorAvatar({ display, size = 16 }: { display: ActorDisplay; size?: number }) {
  const style = { width: size, height: size };
  // The initials follow the avatar on the type scale, as the kit's Avatar does.
  const text = size <= 18 ? "text-[calc(var(--fs-2xs)*0.8)]" : size <= 26 ? "text-2xs" : "text-xs";
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
      className={`grid shrink-0 place-items-center rounded-full bg-primary font-semibold text-primary-ink leading-none ${text}`}
      style={style}
      data-testid="actor-avatar"
    >
      {initials(display.name)}
    </span>
  );
}

