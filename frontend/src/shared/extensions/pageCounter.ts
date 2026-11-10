import { useEffect, useState } from "react";
import type { WsMessage } from "../../run/model/liveStream";
import { subscribe } from "../../run/store/useDaemonSocket";
import type { PageCounter } from "../generated/PageCounter";
import { request } from "../http/client";

/** How long the counter waits for a burst of events to end before it reads. */
export const COUNTER_SETTLE_MS = 150;

/** Whether `msg` is an event of the extension `extension`: one it published,
 *  or one about one of its Sessions hébergées. */
export function isEventOf(msg: WsMessage, extension: string): boolean {
  switch (msg.type) {
    case "extension_event":
    case "hosted_live_artifacts_changed":
      return msg.extension === extension;
    case "hosted_session_changed":
    case "hosted_session_forgotten":
      return msg.session?.extension === extension;
    default:
      return false;
  }
}

/**
 * The counter of a Page's icon (API 0.19, `UiContribution::counter`): what
 * `path` answers for the current Actor, read when shown, again at each event
 * of `extension` on `/ws` and at each resync. `null` without a path, before
 * the first answer, and when it fails (the icon then shows none). It listens
 * to the App's stream without opening one: the top bar lives in the App.
 */
export function usePageCounter(extension: string, path: string | null | undefined): PageCounter | null {
  const [counter, setCounter] = useState<{ path: string; value: PageCounter | null } | null>(null);
  useEffect(() => {
    if (!path) return undefined;
    let alive = true;
    let timer: ReturnType<typeof setTimeout> | null = null;
    const read = () =>
      request<PageCounter>("GET", path, { label: "counter" }).then(
        (value) => alive && setCounter({ path, value }),
        () => alive && setCounter({ path, value: null }),
      );
    const soon = () => {
      if (timer) clearTimeout(timer);
      timer = setTimeout(() => void read(), COUNTER_SETTLE_MS);
    };
    void read();
    const unsubscribe = subscribe((msg) => {
      if (msg.type === "resync" || isEventOf(msg, extension)) soon();
    });
    return () => {
      alive = false;
      if (timer) clearTimeout(timer);
      unsubscribe();
    };
  }, [extension, path]);
  return path && counter?.path === path ? counter.value : null;
}
