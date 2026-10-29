import { useCallback, useMemo, useSyncExternalStore } from "react";
import type { WsMessage } from "../../run/model/liveStream";
import { subscribe, useDaemonSocket } from "../../run/store/useDaemonSocket";
import type { HostedSessionEvent } from "../../shared/generated/HostedSessionEvent";
import { ApiError } from "../../shared/http/client";
import {
  configureHostedSession,
  fetchHostedLiveArtifacts,
  fetchHostedSession,
  fetchHostedSessionEvents,
  relaunchHostedSession,
  terminateHostedSession,
} from "../api/hostedSessions";
import type { HostedConfig, HostedLiveArtifact, HostedSession, HostedSessionView } from "../model/hostedSession";

/** Why the session could not be read: the daemon's status (`403` refused,
 *  `404` no such Session hébergée, or forgotten) and its message. */
export interface HostedSessionError {
  status: number | null;
  message: string;
}

/** What the detail of a session knows of it, live. */
export interface HostedSessionSnapshot {
  /** `null` until read, and once it cannot be (see `error`). */
  session: HostedSessionView | null;
  /** Its Artefacts live, by path. */
  live: HostedLiveArtifact[];
  /** Its journal, oldest first. */
  events: HostedSessionEvent[];
  /** Nothing read yet. */
  loading: boolean;
  error: HostedSessionError | null;
}

/** The state of a Session hébergée and its gestures. */
export interface HostedSessionHandle extends HostedSessionSnapshot {
  /** Terminate it (the Opération `session` × `terminate`). Rejects with the
   *  refusal, its message as the daemon gave it. */
  terminate: () => Promise<HostedSession>;
  /** Relaunch it once ended (`session` × `relaunch`). */
  relaunch: () => Promise<HostedSession>;
  /** Replace its configuration, applied at its next relaunch
   *  (`session` × `configure`). */
  configure: (config: HostedConfig) => Promise<HostedSession>;
}

/** How often who drives its terminal is read again: the daemon publishes no
 *  change of Pilote. */
export const DRIVER_REFRESH_MS = 5_000;

const EMPTY: HostedSessionSnapshot = { session: null, live: [], events: [], loading: true, error: null };

interface Entry {
  snapshot: HostedSessionSnapshot;
  listeners: Set<() => void>;
  detach: () => void;
}

const entries = new Map<string, Entry>();

function errorOf(e: unknown): HostedSessionError {
  if (e instanceof ApiError) return { status: e.status ?? null, message: e.message };
  return { status: null, message: e instanceof Error ? e.message : String(e) };
}

function update(id: string, change: (current: HostedSessionSnapshot) => HostedSessionSnapshot) {
  const entry = entries.get(id);
  if (!entry) return;
  entry.snapshot = change(entry.snapshot);
  for (const listener of entry.listeners) listener();
}

/** Whether `next` is not older than what is known: a read sent before a
 *  gesture may answer after it. */
function notOlder(next: HostedSession, known: HostedSession | null): boolean {
  if (!known) return true;
  const [a, b] = [Date.parse(next.updated_at), Date.parse(known.updated_at)];
  return Number.isNaN(a) || Number.isNaN(b) || a >= b;
}

/** The session as a gesture or `/ws` gives it: who drives it stays known. */
function heard(id: string, session: HostedSession) {
  update(id, (s) =>
    notOlder(session, s.session)
      ? { ...s, session: { ...session, driving: s.session?.driving ?? null }, loading: false, error: null }
      : s,
  );
}

function readView(id: string) {
  return fetchHostedSession(id).then(
    (view) =>
      update(id, (s) => ({
        ...s,
        // An older answer still says who drives it now.
        session: notOlder(view, s.session) ? view : s.session && { ...s.session, driving: view.driving },
        loading: false,
        error: null,
      })),
    (e: unknown) => update(id, (s) => ({ ...s, session: null, loading: false, error: errorOf(e) })),
  );
}

/** Refused or gone: nothing to poll until a resync. */
function settled(id: string): boolean {
  const status = entries.get(id)?.snapshot.error?.status;
  return status === 403 || status === 404;
}

function readEvents(id: string) {
  return fetchHostedSessionEvents(id).then(
    (events) => update(id, (s) => ({ ...s, events })),
    () => undefined,
  );
}

function readLive(id: string) {
  return fetchHostedLiveArtifacts(id).then(
    (live) => update(id, (s) => ({ ...s, live })),
    () => undefined,
  );
}

function readAll(id: string) {
  void readView(id);
  void readEvents(id);
  void readLive(id);
}

function onMessage(id: string, msg: WsMessage) {
  if (msg.type === "resync") {
    readAll(id);
  } else if (msg.type === "hosted_session_changed" && msg.session?.id === id) {
    heard(id, msg.session as HostedSession);
    void readEvents(id);
  } else if (msg.type === "hosted_session_forgotten" && msg.session?.id === id) {
    update(id, (s) => ({ ...s, session: null, live: [], error: { status: 404, message: "This session was forgotten" } }));
  } else if (msg.type === "hosted_live_artifacts_changed" && msg.id === id) {
    update(id, (s) => ({ ...s, live: msg.artifacts ?? [] }));
  }
}

function watch(id: string, listener: () => void) {
  let entry = entries.get(id);
  if (!entry) {
    const unsubscribe = subscribe((msg) => onMessage(id, msg));
    const timer = setInterval(() => {
      if (!settled(id)) void readView(id);
    }, DRIVER_REFRESH_MS);
    entry = {
      snapshot: EMPTY,
      listeners: new Set(),
      detach: () => {
        unsubscribe();
        clearInterval(timer);
      },
    };
    entries.set(id, entry);
    readAll(id);
  }
  entry.listeners.add(listener);
  const watched = entry;
  return () => {
    watched.listeners.delete(listener);
    if (watched.listeners.size === 0 && entries.get(id) === watched) {
      watched.detach();
      entries.delete(id);
    }
  };
}

async function gesture(id: string, run: () => Promise<HostedSession>): Promise<HostedSession> {
  const session = await run();
  heard(id, session);
  void readEvents(id);
  return session;
}

/**
 * A Session hébergée by its id (CORE-125), live: the session and who drives
 * its terminal, its Artefacts live and its journal, read from
 * the daemon then followed on `/ws` (and read again after each resync); and
 * its gestures, each the Opération its extension's would be. Every brick of a
 * session's detail shares one subscription per session.
 */
export function useHostedSession(id: string): HostedSessionHandle {
  // Holds the tab's stream open while a brick shows the session.
  useDaemonSocket();
  const watching = useCallback((listener: () => void) => watch(id, listener), [id]);
  const snapshot = useSyncExternalStore(watching, () => entries.get(id)?.snapshot ?? EMPTY);
  const gestures = useMemo(
    () => ({
      terminate: () => gesture(id, () => terminateHostedSession(id)),
      relaunch: () => gesture(id, () => relaunchHostedSession(id)),
      configure: (config: HostedConfig) => gesture(id, () => configureHostedSession(id, config)),
    }),
    [id],
  );
  return { ...snapshot, ...gestures };
}
