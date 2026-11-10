import { useEffect, useSyncExternalStore } from "react";
import { request } from "../../shared/http/client";
import type { Presence } from "../../shared/generated/Presence";
import type { WsMessage } from "../model/liveStream";
import { showRun, subscribe } from "./useDaemonSocket";

/**
 * The Presences on the Runs (API 0.12): who has which Run open, read once
 * (`GET /runs/presences`) at each resync of the stream, then followed on it
 * (`presence_arrived`, `presence_left`, each carrying the Run's Presences).
 * The `run/item/people` decorations read them in their context.
 */
const byRun = new Map<string, Presence[]>();
const listeners = new Set<() => void>();
const NOBODY: Presence[] = [];
/** The messages received while a reload is in flight: replayed over its
 *  answer, which may predate them. `null` while none is. */
let sinceReload: WsMessage[] | null = null;
/** The latest reload: an older one's answer is dropped. */
let reloads = 0;

function changed() {
  for (const listener of listeners) listener();
}

/** Apply a message of the stream; `true` when it was about the Presences. */
export function applyPresenceMessage(msg: WsMessage): boolean {
  if (msg.type !== "presence_arrived" && msg.type !== "presence_left") return false;
  if (!msg.run_id) return true;
  sinceReload?.push(msg);
  const presences = msg.presences ?? [];
  if (presences.length === 0) byRun.delete(msg.run_id);
  else byRun.set(msg.run_id, presences);
  changed();
  return true;
}

/**
 * Replace every Run's Presences with what the daemon says now, then replay
 * the messages that arrived meanwhile: the answer may have been read before
 * them.
 */
export async function reloadPresences(): Promise<void> {
  const mine = ++reloads;
  sinceReload ??= [];
  let answer: { runs: { run_id: string; presences: Presence[] }[] };
  try {
    answer = await request("GET", "/runs/presences");
  } catch (error) {
    if (mine === reloads) sinceReload = null;
    throw error;
  }
  if (mine !== reloads) return;
  const missed = sinceReload ?? [];
  sinceReload = null;
  byRun.clear();
  for (const { run_id, presences } of answer.runs) byRun.set(run_id, presences);
  for (const msg of missed) applyPresenceMessage(msg);
  changed();
}

function watch(listener: () => void) {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** The Actors who have `runId` open, by order of arrival; live. */
export function useRunPresences(runId: string): Presence[] {
  return useSyncExternalStore(watch, () => byRun.get(runId) ?? NOBODY);
}

/**
 * Keep the Presences live while the App runs, and tell the daemon which Run
 * this tab shows (`runId`, `null` for none).
 */
export function usePresences(runId: string | null) {
  useEffect(
    () =>
      subscribe((msg) => {
        if (msg.type === "resync") void reloadPresences().catch(() => {});
        else applyPresenceMessage(msg);
      }),
    [],
  );
  useEffect(() => {
    showRun(runId);
  }, [runId]);
}
