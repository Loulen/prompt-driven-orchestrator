import { useEffect, useSyncExternalStore } from "react";
import { openProbedSocket, type ProbedSocket, type ProbedSocketState } from "../../shared/http/probedSocket";
import type { WsMessage } from "../model/liveStream";

export type ConnectionStatus = "connected" | "reconnecting" | "disconnected";

/** The `performance` mark of each opening of the event stream. */
export const STREAM_OPEN_MARK = "ws:ouvert";

/** The stream's delay of silence (ADR-0083 §5): the daemon beats `/ws` every
 *  5 s, so three missed beats make a dead socket, closed and probed again. */
export const STREAM_SILENCE_MS = 15_000;

/** The signal to re-read everything (ADR-0083 §6): nothing missed while away
 *  is replayed, so a full re-read is the only way back to the real state. */
export const RESYNC: WsMessage = { type: "resync" };

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
  // A harness's model catalogue probed again (« Refresh models »).
  "harness_catalogue_changed",
  // The validation of a Pipeline changed (CONTEXT.md « Pipeline validé »).
  "pipeline_validation_changed",
  // ADR-0088: who has a Pipeline's shared draft open, and its changes.
  "pipeline_draft_changed",
  // API 0.12: who has which Run open.
  "presence_arrived",
  "presence_left",
  // ADR-0083 §6: the daemon lost messages for this client.
  "resync",
]);

/**
 * The tab's one event stream (`/ws`), shared by everything that listens to
 * it: the App, the Review page (ADR-0083: one stream per tab). Opened, through
 * the probe, by the first user and closed with the last; a socket silent for
 * {@link STREAM_SILENCE_MS} is closed and probed again.
 *
 * Its listeners get {@link RESYNC} after each (re)opening, on a return to the
 * tab, on a return of the network, and when the daemon sends it: they re-read
 * their whole state then.
 */
const listeners = new Set<(msg: WsMessage) => void>();
const statusListeners = new Set<() => void>();
let stream: ProbedSocket | null = null;
let users = 0;
let status: ConnectionStatus = "disconnected";
/** The Run this tab shows (API 0.12), told to the daemon at each opening. */
let shownRun: string | null = null;

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

function emit(msg: WsMessage) {
  for (const listener of listeners) listener(msg);
}

/** Back on the tab, or back online: an open stream re-reads at once (a
 *  silent one was just dropped by the probe, and its reopening re-reads). */
function wake() {
  if (document.visibilityState === "hidden" || status !== "connected") return;
  emit(RESYNC);
}

function acquire() {
  users += 1;
  if (stream) return;
  const socket = openProbedSocket("/ws", { silenceMs: STREAM_SILENCE_MS });
  stream = socket;
  socket.onState((state) => {
    if (state === "ouvert") performance.mark?.(STREAM_OPEN_MARK);
    setStatus(statusOf(state));
    if (state === "ouvert") {
      if (shownRun) tellShownRun();
      emit(RESYNC);
    }
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
    emit(msg);
  });
  document.addEventListener("visibilitychange", wake);
  window.addEventListener("online", wake);
}

function release() {
  users -= 1;
  if (users > 0 || !stream) return;
  stream.close();
  stream = null;
  document.removeEventListener("visibilitychange", wake);
  window.removeEventListener("online", wake);
  setStatus("disconnected");
}

function tellShownRun() {
  stream?.send(JSON.stringify({ type: "show_run", run_id: shownRun }));
}

/**
 * Tell the daemon which Run this tab shows, or `null` for none (API 0.12): the
 * Presences on the Runs. Said again at each (re)opening of the stream; the
 * stream's end is the tab's departure.
 */
export function showRun(runId: string | null) {
  if (runId === shownRun) return;
  shownRun = runId;
  tellShownRun();
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
