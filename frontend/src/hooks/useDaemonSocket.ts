import { useCallback, useEffect, useRef, useState } from "react";
import type { WsMessage } from "../types";

export type ConnectionStatus = "connected" | "reconnecting" | "disconnected";

/** #972: first retry after a drop; each failed attempt doubles it, up to the cap. */
export const RECONNECT_BASE_MS = 1000;
export const RECONNECT_MAX_MS = 30_000;
/**
 * #972: the daemon beats every 5 s. Three missed beats and the socket is deemed
 * dead even if the browser never saw it close (a half-open TCP connection after
 * a network cut, a laptop waking up, a proxy that dropped it silently).
 */
export const HEARTBEAT_TIMEOUT_MS = 15_000;

/** The message the hook hands its listeners when they must re-read everything. */
export const RESYNC: WsMessage = { type: "resync" };

/** Delay before reconnect attempt `attempt` (0-based): 1 s, 2 s, 4 s … capped. */
export function reconnectDelay(attempt: number): number {
  return Math.min(RECONNECT_MAX_MS, RECONNECT_BASE_MS * 2 ** Math.max(0, attempt));
}

// Messages forwarded to listeners. Everything else (`ready`, `heartbeat`, the
// daemon's other broadcasts) only feeds the watchdog.
const FORWARDED = new Set<WsMessage["type"]>([
  "event",
  "pipeline_changed",
  "trigger_created",
  "trigger_fired",
  "trigger_deleted",
  // #348: without this line the global-pause event is silently dropped
  // here before it reaches the App dispatcher, so a second client's
  // banner would never light up. (This transport allowlist is the
  // easiest line in the ticket to miss.)
  "triggers_paused",
  // #972: the daemon lost messages for this client — re-read everything.
  "resync",
]);

/**
 * The daemon's live stream (`/ws`). #972: the UI must be right when the user comes
 * back after an absence, without a reload. So the hook
 * - reconnects with an exponential backoff instead of a fixed retry;
 * - closes and reopens a socket that stopped beating (watchdog);
 * - tells its listeners to re-read everything ({@link RESYNC}) after every
 *   reconnection, every return to the tab, and every `resync` from the daemon:
 *   events missed while away are never replayed, so a re-read is the only way
 *   back to the real state.
 */
export function useDaemonSocket() {
  const [status, setStatus] = useState<ConnectionStatus>("disconnected");
  const listenersRef = useRef<Set<(msg: WsMessage) => void>>(new Set());

  const subscribe = useCallback((listener: (msg: WsMessage) => void) => {
    listenersRef.current.add(listener);
    return () => {
      listenersRef.current.delete(listener);
    };
  }, []);

  useEffect(() => {
    let ws: WebSocket | null = null;
    let disposed = false;
    let attempt = 0;
    // A drop (or a failed first attempt) means events may have been missed: the
    // next `open` must trigger a re-read. The very first open needs none, the
    // App fetches everything on mount.
    let missedEvents = false;
    let lastMessageAt = Date.now();
    let reconnectTimer: ReturnType<typeof setTimeout> | undefined;
    let watchdogTimer: ReturnType<typeof setTimeout> | undefined;

    const emit = (msg: WsMessage) => {
      for (const listener of listenersRef.current) listener(msg);
    };

    const armWatchdog = () => {
      lastMessageAt = Date.now();
      clearTimeout(watchdogTimer);
      watchdogTimer = setTimeout(() => {
        drop();
      }, HEARTBEAT_TIMEOUT_MS);
    };

    const scheduleReconnect = () => {
      clearTimeout(reconnectTimer);
      reconnectTimer = setTimeout(connect, reconnectDelay(attempt));
      attempt += 1;
    };

    // The single exit of a socket: closed, errored or gone silent. It is
    // forgotten first, so its late events (a `close` that arrives after the
    // watchdog already gave up on it) cannot schedule a second reconnect.
    function drop() {
      const current = ws;
      if (!current) return;
      ws = null;
      clearTimeout(watchdogTimer);
      missedEvents = true;
      try {
        current.close();
      } catch {
        // already closed
      }
      if (disposed) return;
      setStatus("reconnecting");
      scheduleReconnect();
    }

    function connect() {
      if (disposed) return;
      clearTimeout(reconnectTimer);
      const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
      const url = `${protocol}//${window.location.host}/ws`;

      const socket = new WebSocket(url);
      ws = socket;

      socket.addEventListener("open", () => {
        if (ws !== socket) return;
        attempt = 0;
        setStatus("connected");
        armWatchdog();
        if (missedEvents) {
          missedEvents = false;
          emit(RESYNC);
        }
      });

      socket.addEventListener("message", (e) => {
        if (ws !== socket) return;
        armWatchdog();
        try {
          const msg: WsMessage = JSON.parse(e.data);
          if (FORWARDED.has(msg.type)) emit(msg);
        } catch {
          // ignore malformed messages
        }
      });

      socket.addEventListener("close", () => {
        if (ws === socket) drop();
      });

      socket.addEventListener("error", () => {
        if (ws === socket) drop();
      });
    }

    // Back on the tab (or back online): a hidden tab's timers are throttled, so
    // the watchdog may not have run — judge the socket now. A silent one is
    // reopened (its `open` re-reads); a live one re-reads at once; a pending
    // retry is brought forward rather than left to its backoff.
    const wake = () => {
      if (disposed) return;
      if (ws && ws.readyState === WebSocket.OPEN) {
        if (Date.now() - lastMessageAt > HEARTBEAT_TIMEOUT_MS) drop();
        else emit(RESYNC);
        return;
      }
      if (!ws) {
        attempt = 0;
        connect();
      }
    };
    const onVisibility = () => {
      if (document.visibilityState === "visible") wake();
    };
    document.addEventListener("visibilitychange", onVisibility);
    window.addEventListener("online", wake);

    connect();
    return () => {
      disposed = true;
      document.removeEventListener("visibilitychange", onVisibility);
      window.removeEventListener("online", wake);
      clearTimeout(reconnectTimer);
      clearTimeout(watchdogTimer);
      const current = ws;
      ws = null;
      current?.close();
    };
  }, []);

  return { status, subscribe };
}
