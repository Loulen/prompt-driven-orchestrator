import type { ActorDisplay } from "../generated/ActorDisplay";
import { initials } from "./initials";

/** A round avatar: the extension's image, or the initials of the name. */
export function ActorAvatar({ display, size = 16 }: { display: ActorDisplay; size?: number }) {
  const style = { width: size, height: size };
  // The initials follow the avatar on the type scale.
  const text = size <= 18 ? "text-2xs" : size <= 26 ? "text-xs" : "text-sm";
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
      className={`grid shrink-0 place-items-center rounded-full bg-primary font-semibold text-primary-ink ${text}`}
      style={style}
      data-testid="actor-avatar"
    >
      {initials(display.name)}
    </span>
  );
}

