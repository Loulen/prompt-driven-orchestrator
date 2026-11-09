import { useEffect, useState } from "react";
import { fetchTrigger } from "../api/triggers";

/**
 * The name of the Trigger `id`, as the user named it, for a surface that only
 * holds its id (a Run's `triggered_by`). `null` until read, and when the Trigger
 * is gone or unreadable: the caller then shows the id.
 */
export function useTriggerName(id: string | null | undefined): string | null {
  const [read, setRead] = useState<{ id: string; name: string } | null>(null);
  useEffect(() => {
    if (!id) return;
    let cancelled = false;
    fetchTrigger(id)
      .then((t) => {
        if (!cancelled) setRead({ id, name: t.name });
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [id]);
  return read && read.id === id ? read.name : null;
}
