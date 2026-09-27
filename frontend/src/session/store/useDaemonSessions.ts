import { useCallback, useState } from "react";
import type { DaemonStatus } from "../../platform/model/daemon";
import { fetchSessions } from "../api/sessions";

/** The status bar's `GET /sessions`, refreshed on demand. A failed read keeps
 *  the last answer. */
export function useDaemonSessions() {
  const [sessions, setSessions] = useState<DaemonStatus>({ live: 0, cap: 0 });

  const refresh = useCallback(async () => {
    try {
      setSessions(await fetchSessions());
    } catch {
      // ignore
    }
  }, []);

  return { sessions, refresh };
}
