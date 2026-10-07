/**
 * The Ouverture sondée (ADR-0083): a WebSocket of the interface opens, and
 * reopens, only after an HTTP request to the same daemon succeeded.
 *
 * Firefox admits one connecting WebSocket per host, every tab and port
 * together, and delays the attempts towards an endpoint that failed: a socket
 * retried blindly towards a stopped daemon held every other tab's sockets for
 * up to a minute. An HTTP request goes through no such queue. So:
 *
 * - the probe (`GET /actors/me`, a short timeout) comes before every opening;
 *   while it fails, the probe is what repeats, spaced by an exponential
 *   backoff with jitter (about 1 s up to about 30 s), reset once a socket opens;
 * - a hidden tab (`document.visibilityState`) does not probe; it probes at
 *   once when it shows again. An open socket stays open whatever the
 *   visibility: an established socket holds no queue;
 * - a session the extensions refuse (`401`) goes to their sign-in page, as
 *   every request does, and no socket is opened;
 * - `close()` closes the socket with 1000.
 *
 * The one implementation: the event stream and the terminals use it, and the
 * extensions' interfaces get it from the shared module `@pdo/extension-ui`.
 */

import { followSignInOf, isRedirectingToSignIn } from "./signIn";

/** Where a probed socket stands: probing or connecting for the first time,
 *  open, or down (probing again, or closed for good). */
export type ProbedSocketState = "connexion" | "ouvert" | "hors_ligne";

/** What a message carries: text as is, binary as an `ArrayBuffer`. */
export type ProbedSocketData = string | ArrayBuffer;

export interface ProbedSocketOptions {
  /**
   * Whether a socket that ended is reopened (after a probe). Default `true`.
   * `false` for a connection that must not come back on its own: it still
   * probes before its one opening. `"after-drop"` (a terminal's PTY) reopens
   * only a socket that was open and dropped without the daemon's Close (the
   * daemon stopped or restarted); a Close from the daemon, or a refused
   * upgrade, ends it for good.
   */
  reconnect?: boolean | "after-drop";
}

/** A WebSocket to the daemon, opened through the probe. */
export interface ProbedSocket {
  /** Its state now. */
  readonly state: ProbedSocketState;
  /** Send `data` when open; whether it was sent. */
  send(data: string | ArrayBuffer | ArrayBufferView<ArrayBuffer> | Blob): boolean;
  /** Listen to every message; returns the unsubscribe. */
  onMessage(listener: (data: ProbedSocketData) => void): () => void;
  /** Listen to every change of state (`ouvert` at each opening); returns the
   *  unsubscribe. */
  onState(listener: (state: ProbedSocketState) => void): () => void;
  /** Close for good, with 1000. */
  close(): void;
}

/** The request the probe sends: who the interface is, as cheap as a request gets. */
export const PROBE_PATH = "/actors/me";
/** How long the probe waits for the daemon. */
export const PROBE_TIMEOUT_MS = 2_000;
/** The first wait after a failure, and the longest. */
export const BACKOFF_MIN_MS = 1_000;
export const BACKOFF_MAX_MS = 30_000;

/** The normal closure. */
const NORMAL_CLOSURE = 1000;
/** The closures of a connection that went away rather than ended: the server
 *  going away, and the browser's own code for a drop without a Close frame. */
const GOING_AWAY = 1001;
const ABNORMAL_CLOSURE = 1006;

type ProbeOutcome = "admitted" | "refused" | "unreachable";

/**
 * The wait before the probe that follows `failures` failures in a row: an
 * exponential backoff from {@link BACKOFF_MIN_MS} capped at
 * {@link BACKOFF_MAX_MS}, of which a random half is taken off so the tabs of
 * a restarted daemon do not probe all at once.
 */
export function backoffDelay(failures: number, random: () => number = Math.random): number {
  const ceiling = Math.min(BACKOFF_MAX_MS, BACKOFF_MIN_MS * 2 ** failures);
  return Math.round(ceiling / 2 + (ceiling / 2) * random());
}

/** Ask the daemon whether it answers and admits the interface. */
async function probe(): Promise<ProbeOutcome> {
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), PROBE_TIMEOUT_MS);
  try {
    const resp = await fetch(PROBE_PATH, { signal: controller.signal, cache: "no-store" });
    if (resp.status === 401) {
      followSignInOf(await resp.json().catch(() => null));
      return "refused";
    }
    // A 5xx is a proxy in front of a stopped daemon.
    return resp.status >= 500 ? "unreachable" : "admitted";
  } catch {
    return "unreachable";
  } finally {
    clearTimeout(timeout);
  }
}

function socketUrl(path: string): string {
  const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
  return `${protocol}//${window.location.host}${path}`;
}

function hidden(): boolean {
  return typeof document !== "undefined" && document.visibilityState === "hidden";
}

/** Open `path` (`/ws`, `/ext/<id>/…`) on the daemon of this page, through the probe. */
export function openProbedSocket(path: string, options: ProbedSocketOptions = {}): ProbedSocket {
  const reconnect = options.reconnect ?? true;
  const messageListeners = new Set<(data: ProbedSocketData) => void>();
  const stateListeners = new Set<(state: ProbedSocketState) => void>();
  let state: ProbedSocketState = "connexion";
  let socket: WebSocket | null = null;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let probing = false;
  let failures = 0;
  // Closed by the caller, or ended for good: nothing happens any more.
  let done = false;
  let closed = false;
  // A probe came due while the tab was hidden.
  let waitingToShow = false;

  const setState = (next: ProbedSocketState) => {
    if (state === next) return;
    state = next;
    for (const listener of stateListeners) listener(next);
  };

  const later = () => {
    setState("hors_ligne");
    clearTimeout(timer);
    timer = setTimeout(attempt, backoffDelay(failures++));
  };

  async function attempt() {
    timer = undefined;
    if (done || socket || probing) return;
    if (hidden()) {
      waitingToShow = true;
      return;
    }
    probing = true;
    const outcome = await probe();
    probing = false;
    if (done) return;
    if (outcome === "admitted") {
      open();
      return;
    }
    // On its way to the sign-in page: nothing more to try from here.
    if (outcome === "refused" && isRedirectingToSignIn()) {
      setState("hors_ligne");
      return;
    }
    later();
  }

  function open() {
    const ws = new WebSocket(socketUrl(path));
    ws.binaryType = "arraybuffer";
    socket = ws;
    ws.addEventListener("open", () => {
      if (socket !== ws) return;
      failures = 0;
      setState("ouvert");
    });
    ws.addEventListener("message", (event: MessageEvent) => {
      if (socket !== ws) return;
      for (const listener of messageListeners) listener(event.data as ProbedSocketData);
    });
    let opened = false;
    ws.addEventListener("open", () => {
      opened = true;
    });
    ws.addEventListener("close", (event: CloseEvent) => {
      if (socket !== ws) return;
      socket = null;
      if (done) return;
      const dropped = opened && (event.code === ABNORMAL_CLOSURE || event.code === GOING_AWAY);
      if (!reconnect || (reconnect === "after-drop" && !dropped)) {
        done = true;
        setState("hors_ligne");
        // Refused at its upgrade (a session that expired since the probe):
        // the probe follows the sign-in page, as it would have.
        if (!opened) void probe();
        return;
      }
      // A refused upgrade or a dropped daemon: the next probe tells which.
      later();
    });
  }

  const onVisibility = () => {
    if (hidden() || done || socket || probing) return;
    // Back in sight: probe now, whether a probe came due while hidden or a
    // backoff is still running.
    if (waitingToShow || timer !== undefined) {
      waitingToShow = false;
      clearTimeout(timer);
      void attempt();
    }
  };
  document.addEventListener("visibilitychange", onVisibility);

  void attempt();

  return {
    get state() {
      return state;
    },
    send(data) {
      if (!socket || socket.readyState !== WebSocket.OPEN) return false;
      socket.send(data);
      return true;
    },
    onMessage(listener) {
      messageListeners.add(listener);
      return () => {
        messageListeners.delete(listener);
      };
    },
    onState(listener) {
      stateListeners.add(listener);
      return () => {
        stateListeners.delete(listener);
      };
    },
    close() {
      if (closed) return;
      closed = true;
      done = true;
      clearTimeout(timer);
      document.removeEventListener("visibilitychange", onVisibility);
      const ws = socket;
      socket = null;
      ws?.close(NORMAL_CLOSURE);
      messageListeners.clear();
      stateListeners.clear();
      state = "hors_ligne";
    },
  };
}
