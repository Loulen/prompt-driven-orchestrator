import { useEffect, useRef } from "react";
import { subscribe, useDaemonStreamStatus } from "./useDaemonSocket";
import { trailingWindow } from "../../shared/lib/trailingWindow";
import type { WsMessage } from "../model/liveStream";

/** How often a view refreshes anyway while the stream is open: the net that
 *  catches what no event announced. */
export const SAFETY_NET_MS = 30_000;

/**
 * Keep a view's read fresh (ADR-0084): while the tab's event stream is open,
 * `load` runs on the messages `matches` keeps (batched by window) and on a slow
 * safety net, never on a poll; while it is not, it runs every `pollMs` as
 * before (`null`: never). The first load is the caller's.
 */
export function useLiveRefresh(
  load: () => void,
  matches: (msg: WsMessage) => boolean,
  pollMs: number | null,
  enabled: boolean,
) {
  const live = useDaemonStreamStatus() === "connected";
  const loadRef = useRef(load);
  const matchesRef = useRef(matches);
  useEffect(() => {
    loadRef.current = load;
    matchesRef.current = matches;
  }, [load, matches]);

  // Back online after a cut: what happened meanwhile came with no event here.
  const wasOffline = useRef(false);
  useEffect(() => {
    if (!enabled) return;
    if (!live) {
      wasOffline.current = true;
      if (pollMs === null) return;
      const timer = setInterval(() => loadRef.current(), pollMs);
      return () => clearInterval(timer);
    }
    if (wasOffline.current) {
      wasOffline.current = false;
      loadRef.current();
    }
    const refresh = trailingWindow(() => loadRef.current());
    const unsubscribe = subscribe((msg) => {
      if (matchesRef.current(msg)) refresh();
    });
    const net = setInterval(() => loadRef.current(), SAFETY_NET_MS);
    return () => {
      unsubscribe();
      refresh.cancel();
      clearInterval(net);
    };
  }, [enabled, live, pollMs]);
}
