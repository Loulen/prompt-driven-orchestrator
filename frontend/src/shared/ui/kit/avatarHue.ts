/**
 * A person's hue: one of the 8 hues of the avatars (`--av-h0` … `--av-h7` in
 * the tokens), picked from its identity, so the same person has the same hue
 * on every surface; and the colour of its marks outside its avatar.
 */
import type { CSSProperties } from "react";
import type { Actor } from "../../generated/Actor";
import { actorKey } from "../../lib/actor";

/** How many hues the avatars share (`--av-h0` … `--av-h7` in the tokens). */
export const AVATAR_HUES = 8;

/** What the hue of a person derives from: its Actor's identity, else its name. */
export function avatarIdentity(name: string, actor?: Actor | null): string {
  return actor ? actorKey(actor) : name;
}

/** The hue of an identity (djb2 of its lowercased text), an index into the {@link AVATAR_HUES} hues. */
export function avatarHue(identity: string): number {
  let h = 5381;
  for (const c of identity.trim().toLowerCase()) h = (Math.imul(h, 33) + c.codePointAt(0)!) >>> 0;
  return h % AVATAR_HUES;
}

/** A person as its marks know it: its Actor, else its name. */
export interface Person {
  name?: string;
  actor?: Actor | null;
}

/** The colour of a person's marks outside its avatar (a presence outline, a name tag): its avatar's hue, solid. */
export function personColor({ name = "", actor }: Person): string {
  return `oklch(var(--av-solid-l) var(--av-solid-c) var(--av-h${avatarHue(avatarIdentity(name, actor))}))`;
}

/** The person's colour as the `--pc` its marks paint with. */
export function personStyle(person: Person): CSSProperties {
  return { ["--pc" as string]: personColor(person) };
}
