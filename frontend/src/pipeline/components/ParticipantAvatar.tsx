import type { DraftParticipant } from "../../shared/generated/DraftParticipant";
import { initials } from "../../shared/actors/initials";
import { firstName, participantName, presenceColor } from "../model/presence";

/**
 * A participant of a shared draft (ADR-0088): the extension's image or the
 * initials of its name, ringed in its color; its name (and whether it only
 * reads) on hover.
 */
export function ParticipantAvatar({ participant, size = 16 }: { participant: DraftParticipant; size?: number }) {
  const color = presenceColor(participant);
  const name = participantName(participant);
  const title = participant.can_write ? name : `${name} (read only)`;
  const style = {
    width: size,
    height: size,
    fontSize: Math.max(7, Math.round(size * 0.42)),
    boxShadow: `0 0 0 1.5px ${color}`,
  };
  return participant.display?.avatar_url ? (
    <img
      src={participant.display.avatar_url}
      alt={name}
      title={title}
      className="shrink-0 rounded-full object-cover"
      style={style}
      data-testid="draft-participant"
    />
  ) : (
    <span
      title={title}
      aria-label={title}
      className="grid shrink-0 place-items-center rounded-full font-semibold text-bg-0"
      style={{ ...style, background: color }}
      data-testid="draft-participant"
    >
      {initials(name)}
    </span>
  );
}

/** The participants side by side, overlapping a little. */
export function ParticipantStack({ participants, size = 16 }: { participants: DraftParticipant[]; size?: number }) {
  if (participants.length === 0) return null;
  return (
    <span className="flex items-center" data-testid="draft-participants">
      {participants.map((p, i) => (
        <span key={p.connection} style={{ marginLeft: i === 0 ? 0 : -4, zIndex: participants.length - i }}>
          <ParticipantAvatar participant={p} size={size} />
        </span>
      ))}
    </span>
  );
}

/**
 * The first name of the participant who selected a canvas element, above
 * its top-left corner, in its color. No mouse cursor follows anyone: the
 * selection is what the others see.
 */
export function SelectedByLabel({ participant }: { participant: DraftParticipant }) {
  return (
    <span
      className="pointer-events-none absolute z-[5] whitespace-nowrap rounded-t px-1 font-medium text-bg-0"
      style={{ top: -15, left: -1.5, fontSize: "9.5px", lineHeight: "14px", background: presenceColor(participant) }}
      data-testid="selected-by"
    >
      {firstName(participant)}
    </span>
  );
}
