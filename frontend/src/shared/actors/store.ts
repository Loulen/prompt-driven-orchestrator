/**
 * The Actors as a person sees them (ADR-0081, 0.8): who uses the UI
 * (`GET /actors/me`), and the display name the extensions give any Actor
 * (`POST /actors/display`, batched and cached). Without an extension that
 * names Actors, every display is `null` and the UI shows what it always did.
 */

import { useEffect, useMemo } from "react";
import { create } from "zustand";
import type { Actor } from "../generated/Actor";
import type { ActorDisplay } from "../generated/ActorDisplay";
import type { ActorsDisplayResponse } from "../generated/ActorsDisplayResponse";
import type { CurrentActor } from "../generated/CurrentActor";
import { request } from "../http/client";
import { actorKey } from "../lib/actor";

export { actorKey };

interface ActorsState {
  /** The UI's own Actor; `null` until known. */
  current: CurrentActor | null;
  /** Display names by {@link actorKey}: `null` = no extension names it. */
  displays: Record<string, ActorDisplay | null>;
  setCurrent: (current: CurrentActor) => void;
  remember: (actors: Actor[], displays: (ActorDisplay | null)[]) => void;
}

export const useActorsStore = create<ActorsState>((set) => ({
  current: null,
  displays: {},
  setCurrent: (current) =>
    set((state) => ({
      current,
      displays: { ...state.displays, [actorKey(current.actor)]: current.display },
    })),
  remember: (actors, displays) =>
    set((state) => {
      const next = { ...state.displays };
      actors.forEach((actor, i) => {
        next[actorKey(actor)] = displays[i] ?? null;
      });
      return { displays: next };
    }),
}));

/** `GET /actors/me`, kept in the store. Throws the {@link request} error. */
export async function loadCurrentActor(): Promise<CurrentActor> {
  const current = await request<CurrentActor>("GET", "/actors/me");
  useActorsStore.getState().setCurrent(current);
  return current;
}

let pending = new Map<string, Actor>();
let flush: Promise<void> | null = null;

/** Ask the daemon for the display names of `actors` not asked yet, batched
 *  with the other asks of the same tick. */
export function requestDisplays(actors: Actor[]): Promise<void> {
  const known = useActorsStore.getState().displays;
  for (const actor of actors) {
    const key = actorKey(actor);
    if (!(key in known)) pending.set(key, actor);
  }
  if (pending.size === 0) return Promise.resolve();
  flush ??= Promise.resolve().then(async () => {
    const batch = [...pending.values()];
    pending = new Map();
    flush = null;
    try {
      const answer = await request<ActorsDisplayResponse>("POST", "/actors/display", {
        body: { actors: batch },
      });
      useActorsStore.getState().remember(batch, answer.displays);
    } catch {
      // Unnamed: the UI shows the Actor as it would without extension.
      useActorsStore.getState().remember(batch, batch.map(() => null));
    }
  });
  return flush;
}

/** The UI's own Actor and its display name; `null` until known. */
export function useCurrentActor(): CurrentActor | null {
  return useActorsStore((state) => state.current);
}

/** The display name of `actor`; `null` while unknown, or when no extension
 *  names it. */
export function useActorDisplay(actor: Actor | null | undefined): ActorDisplay | null {
  const key = actor ? actorKey(actor) : null;
  const display = useActorsStore((state) => (key ? state.displays[key] : undefined));
  useEffect(() => {
    if (actor && display === undefined) void requestDisplays([actor]);
    // `key` stands for `actor`.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, display]);
  return display ?? null;
}

/**
 * The display names of `actors`, in their order (`null` while unknown, or
 * when no extension names it); those not known yet are asked for, batched.
 * One call for a list of any length, a stack of avatars say.
 */
export function useActorDisplays(actors: readonly Actor[]): (ActorDisplay | null)[] {
  const displays = useActorsStore((state) => state.displays);
  const keys = actors.map(actorKey);
  const missing = actors.filter((_, i) => !(keys[i] in displays));
  const ask = missing.map(actorKey).join("\n");
  useEffect(() => {
    if (missing.length > 0) void requestDisplays(missing);
    // `ask` stands for `missing`.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ask]);
  const list = keys.join("\n");
  // eslint-disable-next-line react-hooks/exhaustive-deps
  return useMemo(() => keys.map((key) => displays[key] ?? null), [displays, list]);
}

/** Tests: forget everything. */
export function resetActorsForTests(): void {
  pending = new Map();
  flush = null;
  useActorsStore.setState({ current: null, displays: {} });
}
