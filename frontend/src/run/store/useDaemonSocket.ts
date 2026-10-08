import { useEffect, useSyncExternalStore } from "react";
import { openProbedSocket, type ProbedSocket, type ProbedSocketState } from "../../shared/http/probedSocket";
import type { WsMessage } from "../model/liveStream";

export type ConnectionStatus = "connected" | "reconnecting" | "disconnected";

/** The `performance` mark of each opening of the event stream. */
export const STREAM_OPEN_MARK = "ws:ouvert";

/** The messages the stream hands its subscribers. */
const RELAYED: ReadonlySet<WsMessage["type"]> = new Set<WsMessage["type"]>([
  "event",
  "pipeline_changed",
  "trigger_created",
  "trigger_fired",
  // A Trigger edited elsewhere, or all of them when an extension signals that
  // their suspensions changed (no `trigger_id`): the list reads them again.
  "trigger_updated",
  "trigger_deleted",
  // #348: without this one the global-pause event is silently dropped here
  // before it reaches the App dispatcher, so a second client's banner would
  // never light up. (This transport allowlist is the easiest line in the
  // ticket to miss.)
  "triggers_paused",
  // #552: a Projet created, renamed or deleted elsewhere.
  "project_changed",
  // The validation of a Pipeline changed (CONTEXT.md « Pipeline validé »).
  "pipeline_validation_changed",
]);

/**
 * The tab's one event stream (`/ws`), shared by everything that listens to
 * it: the App, the Review page (ADR-0083: one stream per tab). Opened, through
 * the probe, by the first user and closed with the last.
 */
const listeners = new Set<(msg: WsMessage) => void>();
const statusListeners = new Set<() => void>();
let stream: ProbedSocket | null = null;
let users = 0;
let status: ConnectionStatus = "disconnected";

function statusOf(state: ProbedSocketState): ConnectionStatus {
  switch (state) {
    case "ouvert":
      return "connected";
    case "hors_ligne":
      return "reconnecting";
    case "connexion":
      return "disconnected";
  }
}

function setStatus(next: ConnectionStatus) {
  status = next;
  for (const listener of statusListeners) listener();
}

function acquire() {
  users += 1;
  if (stream) return;
  const socket = openProbedSocket("/ws");
  stream = socket;
  socket.onState((state) => {
    if (state === "ouvert") performance.mark?.(STREAM_OPEN_MARK);
    setStatus(statusOf(state));
  });
  socket.onMessage((data) => {
    if (typeof data !== "string") return;
    let msg: WsMessage;
    try {
      msg = JSON.parse(data);
    } catch {
      return; // ignore malformed messages
    }
    if (!RELAYED.has(msg.type)) return;
    for (const listener of listeners) listener(msg);
  });
}

function release() {
  users -= 1;
  if (users > 0 || !stream) return;
  stream.close();
  stream = null;
  setStatus("disconnected");
}

/** Listen to the stream's messages; returns the unsubscribe. */
export function subscribe(listener: (msg: WsMessage) => void) {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

function watchStatus(listener: () => void) {
  statusListeners.add(listener);
  return () => {
    statusListeners.delete(listener);
  };
}

/** The stream's status, without opening it: for a view inside the App, which
 *  holds the stream. */
export function useDaemonStreamStatus(): ConnectionStatus {
  return useSyncExternalStore(watchStatus, () => status);
}

/** The tab's event stream: its status and its subscription. Every caller
 *  shares one socket. */
export function useDaemonSocket() {
  const current = useSyncExternalStore(watchStatus, () => status);
  useEffect(() => {
    acquire();
    return release;
  }, []);
  return { status: current, subscribe };
}
