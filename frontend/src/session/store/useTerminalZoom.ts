import { useCallback, useEffect, useState } from "react";
import { useDaemonSocket } from "../../run/store/useDaemonSocket";
import { ApiError } from "../../shared/http/client";
import { fetchTerminalZoom, putTerminalZoom } from "../api/sessions";
import { clampTerminalZoom, DEFAULT_TERMINAL_ZOOM } from "../model/terminalZoom";

/** The Zoom du terminal of one session, live. */
export interface TerminalZoomState {
  /** In percent; 100 for a session that carries none. */
  zoom: number;
  /** Whether the session carries a Zoom du terminal (a Session hébergée):
   *  only then can it be set. `null` until the daemon answered. */
  hosted: boolean | null;
  /** Set it for every Poste on the session. Rejects with the daemon's refusal
   *  (a refused Operation, a session that is not hosted); the zoom shown
   *  goes back to the session's. */
  setZoom: (zoom: number) => Promise<void>;
}

/**
 * The Zoom du terminal of `session` (CORE-124): read from the daemon, then
 * followed on `/ws` (`hosted_session_changed`), so a change made from any
 * Poste, by the Host or the API reaches every terminal that shows it. Read
 * again after each resync of the stream.
 */
export function useTerminalZoom(session: string): TerminalZoomState {
  const { subscribe } = useDaemonSocket();
  // Keyed by the session it is about: another session starts at 100 %, unread.
  const [heard, setHeard] = useState<{ session: string; zoom: number; hosted: boolean | null }>({
    session,
    zoom: DEFAULT_TERMINAL_ZOOM,
    hosted: null,
  });
  // A change being sent, shown until the daemon answers.
  const [pending, setPending] = useState<{ session: string; zoom: number } | null>(null);

  useEffect(() => {
    let alive = true;
    const hear = (zoom: number, hosted: boolean | null) =>
      alive && setHeard({ session, zoom: clampTerminalZoom(zoom), hosted });
    const read = () =>
      fetchTerminalZoom(session)
        .then((answer) => hear(answer.zoom, answer.hosted))
        // Refused: this Actor may not read it, nor set it. Unreachable (the
        // daemon away): unknown until the next resync. 100 % meanwhile.
        .catch((e: unknown) => {
          if (e instanceof ApiError && e.status === 403) hear(DEFAULT_TERMINAL_ZOOM, false);
        });
    void read();
    const unsubscribe = subscribe((msg) => {
      if (msg.type === "resync") void read();
      if (msg.type === "hosted_session_changed" && msg.session?.id === session) {
        hear(msg.session.terminal_zoom ?? DEFAULT_TERMINAL_ZOOM, true);
      }
    });
    return () => {
      alive = false;
      unsubscribe();
    };
  }, [session, subscribe]);

  const setZoom = useCallback(
    async (next: number) => {
      const wanted = clampTerminalZoom(next);
      setPending({ session, zoom: wanted });
      try {
        const answer = await putTerminalZoom(session, wanted);
        setHeard({ session, zoom: clampTerminalZoom(answer.zoom), hosted: answer.hosted });
      } finally {
        setPending((current) => (current?.session === session && current.zoom === wanted ? null : current));
      }
    },
    [session],
  );

  const current = heard.session === session ? heard : { zoom: DEFAULT_TERMINAL_ZOOM, hosted: null };
  const zoom = pending?.session === session ? pending.zoom : current.zoom;
  return { zoom, hosted: current.hosted, setZoom };
}
