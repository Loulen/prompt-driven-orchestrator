import { useCallback, useState } from "react";
import type { TriggerListEntry } from "../../shared/generated/TriggerListEntry";
import { fetchTriggers } from "../api/triggers";

/** The Triggers list, refreshed on demand (mount, and every `trigger_*` WS push). */
export function useTriggers() {
  const [triggers, setTriggers] = useState<TriggerListEntry[]>([]);

  const refresh = useCallback(async () => {
    try {
      setTriggers(await fetchTriggers());
    } catch {
      // ignore
    }
  }, []);

  return { triggers, refresh };
}
