/**
 * The participants of a shared draft as the editor draws them (ADR-0088): the
 * hue of their avatar (their Actor's), and a first name.
 */

import type { CSSProperties } from "react";
import type { DraftParticipant } from "../../shared/generated/DraftParticipant";
import { actorLabel } from "../../shared/lib/actor";
import { personColor, personStyle } from "../../shared/ui/kit";

/**
 * The participant's colour: its avatar's hue, the same on every surface. A
 * library assistant takes its Actor's.
 */
export function presenceColor(participant: Pick<DraftParticipant, "actor">): string {
  return personColor({ actor: participant.actor });
}

/** The participant's colour, as the `--pc` its marks paint with. */
export function presenceStyle(participant: Pick<DraftParticipant, "actor">): CSSProperties {
  return personStyle({ actor: participant.actor });
}

/**
 * Its name: the extension's display name, else the core's label; a library
 * assistant is its Actor's.
 */
export function participantName(participant: DraftParticipant): string {
  const name = participant.display?.name ?? actorLabel(participant.actor);
  return participant.assistant ? `${name}'s library assistant` : name;
}

/**
 * Its first name, as the canvas labels it (« Alice is typing »); « Library »
 * for a library assistant.
 */
export function firstName(participant: DraftParticipant): string {
  if (participant.assistant) return "Library";
  const name = participantName(participant).trim();
  return name.split(/\s+/)[0] || name;
}

/** The participants other than the connection `mine`. */
export function others(participants: DraftParticipant[], mine: string | null): DraftParticipant[] {
  return participants.filter((p) => p.connection !== mine);
}
