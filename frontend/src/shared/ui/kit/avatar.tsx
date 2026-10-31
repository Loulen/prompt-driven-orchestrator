/**
 * The one avatar of the shared design system: round, a tint of the person's
 * hue, the initials in ink. Every surface that shows a person draws it, so the
 * same person has the same hue and the same initials everywhere. Styles in
 * kit.css, hues and lightness in the tokens (`--av-*`).
 */
import type { HTMLAttributes } from "react";
import { Bot, Zap } from "lucide-react";
import type { Actor } from "../../generated/Actor";
import { avatarHue, avatarIdentity } from "./avatarHue";
import { cx, initials } from "./util";

export type AvatarSize = 16 | 20 | 24;

export interface AvatarProps extends Omit<HTMLAttributes<HTMLSpanElement>, "title" | "children" | "role"> {
  name: string;
  /** Who it is: the hue follows the Actor (its name without one), and an agent or a trigger shows its glyph. */
  actor?: Actor | null;
  /** An agent shows a robot, a trigger a bolt, both on a neutral fill. By default, the Actor's kind. */
  kind?: "human" | "agent" | "trigger";
  /** 16 in rows, 20 in bars and presence, 24 in a profile. */
  size?: AvatarSize;
  /** The image an extension gives the person, if any. */
  src?: string | null;
  /** A deactivated account, or a person who left: dashed, faint. */
  off?: boolean;
  /** Reads only: no fill, a ring in the person's hue. */
  hollow?: boolean;
  /** Has it open now: a ring in the done colour. */
  present?: boolean;
  /** Drawn over the previous avatar of a stack. */
  stacked?: boolean;
  /** Tooltip and accessible name; the name by default. */
  title?: string;
  /** `data-testid` and the like, on the avatar. */
  [data: `data-${string}`]: string | undefined;
}

function kindOf(actor: Actor | null | undefined): AvatarProps["kind"] {
  return actor?.kind === "agent" ? "agent" : actor?.kind === "trigger" ? "trigger" : "human";
}

/** A person: round, the tint of its hue, its initials; a robot or a bolt for an agent or a trigger. */
export function Avatar({
  name,
  actor,
  kind = kindOf(actor),
  size = 20,
  src,
  off,
  hollow,
  present,
  stacked,
  title,
  className,
  ...rest
}: AvatarProps) {
  const ic = Math.round(size * 0.62);
  const label = title ?? name;
  return (
    <span
      {...rest}
      className={cx("d-av", className)}
      data-size={size}
      data-kind={kind}
      data-hue={kind === "human" ? avatarHue(avatarIdentity(name, actor)) : undefined}
      data-off={off || undefined}
      data-hollow={hollow || undefined}
      data-present={present || undefined}
      data-stacked={stacked || undefined}
      title={label}
      role="img"
      aria-label={label}
    >
      {src && kind === "human" ? (
        <img src={src} alt="" />
      ) : kind === "agent" ? (
        <Bot size={ic} strokeWidth={1.75} aria-hidden />
      ) : kind === "trigger" ? (
        <Zap size={ic} strokeWidth={1.75} aria-hidden />
      ) : (
        initials(name)
      )}
    </span>
  );
}

/** One avatar of a stack; `key` tells two people of the same name apart. */
export type StackPerson = Omit<AvatarProps, "size" | "stacked"> & { key?: string };

export interface AvatarStackProps extends Omit<HTMLAttributes<HTMLSpanElement>, "title" | "children"> {
  people: StackPerson[];
  size?: AvatarSize;
  /** How many avatars before « +n »; 3 by default. */
  max?: number;
  /** Tooltip of the whole stack. */
  title?: string;
}

/**
 * People overlapping by an eighth: the first `max`, then « +n », whose
 * tooltip lists the others, one per line. Nothing without anybody.
 */
export function AvatarStack({ people, size = 16, max = 3, title, className, ...attributes }: AvatarStackProps) {
  if (people.length === 0) return null;
  const rest = people.slice(max);
  return (
    <span {...attributes} className={cx("d-avstack", className)} title={title}>
      {people.slice(0, max).map(({ key, ...p }, i) => (
        <Avatar key={key ?? avatarIdentity(p.name, p.actor)} {...p} size={size} stacked={i > 0} />
      ))}
      {rest.length > 0 && (
        <span
          className="d-avmore"
          data-size={size}
          title={rest.map((p) => p.title ?? p.name).join("\n")}
          role="img"
          aria-label={`${rest.length} more: ${rest.map((p) => p.name).join(", ")}`}
        >
          +{rest.length}
        </span>
      )}
    </span>
  );
}
