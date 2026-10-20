/**
 * The participants of a shared draft as the editor draws them (ADR-0088): a
 * color each (the daemon deals them out), and a first name.
 */

import type { DraftParticipant } from "../../shared/generated/DraftParticipant";
import { actorLabel } from "../../shared/lib/actor";

/** One color per participant, by the index the daemon gave it. */
export const PRESENCE_COLORS = [
  "#a78bfa",
  "#38bdf8",
  "#f472b6",
  "#fbbf24",
  "#34d399",
  "#fb923c",
  "#2dd4bf",
  "#f87171",
] as const;

export function presenceColor(participant: Pick<DraftParticipant, "color">): string {
  return PRESENCE_COLORS[participant.color % PRESENCE_COLORS.length];
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
