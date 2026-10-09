import type { ActorDisplay } from "../generated/ActorDisplay";
import { initials } from "./initials";

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

