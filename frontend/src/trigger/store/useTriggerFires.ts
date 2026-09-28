import { useEffect, useState } from "react";
import type { TriggerFire } from "../../shared/generated/TriggerFire";
import { fetchTriggerFires } from "../api/triggers";

/**
 * A Trigger's fire history, refetched when the Trigger changes or the parent
 * bumps `refreshKey` (every `trigger_fired` WS message, #341).
 */
export function useTriggerFires(triggerId: string, refreshKey?: number) {
  const [fires, setFires] = useState<TriggerFire[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;
    // Bounded cascade: this effect only re-runs when `triggerId` (or the
    // parent's fire counter, #341) changes, so resetting the loading flag here
    // does not loop.
    // eslint-disable-next-line react-hooks/set-state-in-effect -- see note above.
    setLoading(true);
    fetchTriggerFires(triggerId)
      .then((rows) => {
        if (!cancelled) setFires(rows);
      })
      .catch(() => {
        if (!cancelled) setFires([]);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [triggerId, refreshKey]);

  return { fires, loading };
}
