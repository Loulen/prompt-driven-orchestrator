/**
 * The words of a notice of a shared draft (ADR-0088): who or what changed it
 * from outside the canvas, and which fields.
 */

import type { Actor } from "../../shared/generated/Actor";
import type { DraftNotice } from "../../shared/generated/DraftNotice";
import type { DraftParticipant } from "../../shared/generated/DraftParticipant";
import { actorLabel } from "../../shared/lib/actor";

/** The fields named in full; the rest are counted. */
const FIELDS_NAMED = 4;

/** « a, b and c », the fields past {@link FIELDS_NAMED} counted. */
export function fieldsLabel(fields: string[]): string {
  if (fields.length === 0) return "";
  const named = fields.slice(0, FIELDS_NAMED);
  const rest = fields.length - named.length;
  if (rest > 0) return `${named.join(", ")} and ${rest} more`;
  if (named.length === 1) return named[0];
  return `${named.slice(0, -1).join(", ")} and ${named[named.length - 1]}`;
}

function sameActor(a: Actor, b: Actor): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

/** The Actor of this tab's connection among the participants. */
export function myActor(participants: DraftParticipant[], connection: string | null): Actor | null {
  return participants.find((p) => p.connection === connection)?.actor ?? null;
}

/** Whose library assistant changed the draft: « Your » or « Alice's ». */
export function assistantOwner(notice: Extract<DraftNotice, { kind: "assistant" }>, me: Actor | null): string {
  if (me && sameActor(notice.actor, me)) return "Your";
  return `${notice.display?.name ?? actorLabel(notice.actor)}'s`;
}
