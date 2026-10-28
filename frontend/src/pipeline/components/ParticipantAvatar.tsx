import { Bot } from "lucide-react";
import type { DraftParticipant } from "../../shared/generated/DraftParticipant";
import { initials } from "../../shared/actors/initials";
import { firstName, participantName, presenceStyle } from "../model/presence";
import "./inspect.css";

/**
 * A participant of a shared draft (ADR-0088): the extension's image or the
 * initials of its name in its color; a robot for a library assistant; hollow
 * when it only reads. Its name (and whether it only reads) on hover.
 */
export function ParticipantAvatar({ participant, size = "md" }: { participant: DraftParticipant; size?: "sm" | "md" }) {
  const name = participantName(participant);
  const title = participant.can_write ? name : `${name} (read only)`;
  return (
    <span
      title={title}
      aria-label={title}
      role="img"
      className="pi-av"
      style={presenceStyle(participant)}
      data-size={size}
      data-ro={participant.can_write ? undefined : true}
      data-testid="draft-participant"
      data-assistant={participant.assistant ? "true" : undefined}
    >
      {participant.assistant ? (
        <Bot size={size === "sm" ? 9 : 11} strokeWidth={2} aria-hidden />
      ) : participant.display?.avatar_url ? (
        <img src={participant.display.avatar_url} alt="" />
      ) : (
        initials(name)
      )}
    </span>
  );
}

/** One avatar per person: someone in two sessions is still one participant. */
function onePerPerson(participants: DraftParticipant[]): DraftParticipant[] {
  const seen = new Set<string>();
  return participants.filter((p) => {
    const key = p.assistant ? `connection:${p.connection}` : JSON.stringify(p.actor);
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}

/** The participants side by side, overlapping a little; past `max`, a count. */
export function ParticipantStack({
  participants,
  size = "md",
  max = 4,
}: {
  participants: DraftParticipant[];
  size?: "sm" | "md";
  max?: number;
}) {
  const people = onePerPerson(participants);
  if (people.length === 0) return null;
  const shown = people.slice(0, max);
  const rest = people.slice(max);
  return (
    <span className="pi-stack" data-testid="draft-participants">
      {shown.map((p) => (
        <ParticipantAvatar key={p.connection} participant={p} size={size} />
      ))}
      {rest.length > 0 && (
        <span className="pi-stack__more" title={rest.map(participantName).join(", ")}>
          +{rest.length}
        </span>
      )}
    </span>
  );
}

/**
 * The first name of the participant who selected a canvas element, on a tag
 * above its top-left corner, in its color. No mouse cursor follows anyone:
 * the selection is what the others see.
 */
export function SelectedByLabel({ participant }: { participant: DraftParticipant }) {
  return (
    <span
      className="pi-tag"
      style={presenceStyle(participant)}
      title={participantName(participant)}
      data-testid="selected-by"
    >
      {participant.assistant && <Bot size={10} strokeWidth={2} aria-hidden />}
      {firstName(participant)}
    </span>
  );
}
