/**
 * The participants of a shared draft as the editor draws them (ADR-0088): a
 * color each (the daemon deals them out), and a first name.
 */

import type { CSSProperties } from "react";
import type { DraftParticipant } from "../../shared/generated/DraftParticipant";
import { actorLabel } from "../../shared/lib/actor";

/** How many colors the participants share (`--presence-0` … in `inspect.css`). */
export const PRESENCE_COLOR_COUNT = 8;

/** One color per participant, by the index the daemon gave it: a theme token. */
export function presenceColor(participant: Pick<DraftParticipant, "color">): string {
  return `var(--presence-${participant.color % PRESENCE_COLOR_COUNT})`;
}

/** The participant's color, as the `--pc` its marks paint with. */
export function presenceStyle(participant: Pick<DraftParticipant, "color">): CSSProperties {
  return { ["--pc" as string]: presenceColor(participant) };
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
