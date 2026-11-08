import { useCallback, useEffect, useRef, useState } from "react";
import type { RunState } from "../../shared/generated/RunState";
import { fetchRun } from "../api/runs";
import { isLiveRun } from "../model/runStatus";
import { SAFETY_NET_MS } from "./useLiveRefresh";

/**
 * The Run the user has selected, fetched on selection and refetched on
 * demand (its events). A late answer for a Run the user has since left is
 * dropped. A live Run is also read again on the safety net: its costs (a
 * node's, a loop's against its cap, CORE-217) climb while an agent works,
 * and no event says so.
 */
export function useSelectedRun() {
  const [run, setRun] = useState<RunState | null>(null);
  const currentIdRef = useRef<string | null>(null);

  const refresh = useCallback(async () => {
    const id = currentIdRef.current;
    if (!id) return;
    try {
      const data = await fetchRun(id);
      if (currentIdRef.current === id) setRun(data);
    } catch {
      // ignore
    }
  }, []);

  const select = useCallback(
    (newId: string | null) => {
      currentIdRef.current = newId;
      if (!newId) {
        setRun(null);
        return;
      }
      fetchRun(newId)
        .then((data) => {
          if (currentIdRef.current === newId) setRun(data);
        })
        .catch(() => {});
    },
    [],
  );

  const liveId = run && isLiveRun(run.status) ? run.run_id : null;
  useEffect(() => {
    if (!liveId) return;
    const net = setInterval(() => void refresh(), SAFETY_NET_MS);
    return () => clearInterval(net);
  }, [liveId, refresh]);

  return { run, select, refresh };
}
