import { useCallback, useEffect, useState } from "react";
import { fetchUpdateStatus } from "../api";
import { UPDATE_STATUS_CHANGED, announceUpdateStatus } from "../lib/updateStatus";
import type { UpdateStatus } from "../types";

/**
 * The daemon's cached version-check state (#697), shared by the status-bar badge and
 * the Settings section. One fetch on mount (+ `refresh` for the caller's own moments:
 * WebSocket reconnect, after « Check now »), and every consumer converges through the
 * `pdo:update-status-changed` bus: a section that just toggled the check announces the
 * fresh status, and the badge follows without its own round-trip.
 */
export function useUpdateStatus(enabled: boolean = true) {
  const [status, setStatus] = useState<UpdateStatus | null>(null);

  const refresh = useCallback(async () => {
    try {
      const fresh = await fetchUpdateStatus();
      setStatus(fresh);
      announceUpdateStatus(fresh);
      return fresh;
    } catch {
      // The read path never blocks on the source; a failed read keeps the last value.
      return null;
    }
  }, []);

  useEffect(() => {
    if (!enabled) return;
    let cancelled = false;
    fetchUpdateStatus()
      .then((fresh) => {
        if (!cancelled) setStatus(fresh);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [enabled]);

  useEffect(() => {
    const onChange = (e: Event) => setStatus((e as CustomEvent<UpdateStatus>).detail);
    window.addEventListener(UPDATE_STATUS_CHANGED, onChange);
    return () => window.removeEventListener(UPDATE_STATUS_CHANGED, onChange);
  }, []);

  return { status, setStatus, refresh };
}
