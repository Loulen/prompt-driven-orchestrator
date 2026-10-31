import { Bot } from "lucide-react";
import type { DraftParticipant } from "../../shared/generated/DraftParticipant";
import { Avatar, AvatarStack, type AvatarProps } from "../../shared/ui/kit";
import { firstName, participantName, presenceStyle } from "../model/presence";
import "./inspect.css";

/**
 * A participant of a shared draft (ADR-0088), as the kit's avatar: the
 * extension's image or its initials in its hue; a robot for a library
 * assistant; hollow when it only reads. Its name (and whether it only reads)
 * on hover.
 */
function avatarOf(participant: DraftParticipant): AvatarProps {
  const name = participantName(participant);
  return {
    name,
    actor: participant.actor,
    kind: participant.assistant ? "agent" : "human",
    src: participant.display?.avatar_url,
    hollow: !participant.can_write,
    title: participant.can_write ? name : `${name} (read only)`,
    "data-testid": "draft-participant",
    "data-assistant": participant.assistant ? "true" : undefined,
  };
}

/** 16 px in a row, 20 px over the canvas. */
const PX = { sm: 16, md: 20 } as const;

export function ParticipantAvatar({ participant, size = "md" }: { participant: DraftParticipant; size?: "sm" | "md" }) {
  return <Avatar {...avatarOf(participant)} size={PX[size]} />;
}

/**
 * One avatar per person: someone in two sessions is still one participant,
 * drawn as the session that writes when one does.
 */
function onePerPerson(participants: DraftParticipant[]): DraftParticipant[] {
  const byPerson = new Map<string, DraftParticipant>();
  for (const p of participants) {
    const key = p.assistant ? `connection:${p.connection}` : JSON.stringify(p.actor);
    const seen = byPerson.get(key);
    if (!seen || (!seen.can_write && p.can_write)) byPerson.set(key, p);
  }
  return [...byPerson.values()];
}

/** The participants overlapping, three of them, then « +n » naming the others. */
export function ParticipantStack({
  participants,
  size = "md",
  max = 3,
}: {
  participants: DraftParticipant[];
  size?: "sm" | "md";
  max?: number;
}) {
  return (
    <AvatarStack
      people={onePerPerson(participants).map((p) => ({ ...avatarOf(p), key: p.connection }))}
      size={PX[size]}
      max={max}
      data-testid="draft-participants"
    />
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
