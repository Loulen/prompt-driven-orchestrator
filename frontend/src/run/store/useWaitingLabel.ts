import { useEffect, useState } from "react";

/** « waiting 12 min » — ticks every 30 s; `null` before the first tick is « … ». */
export function useWaitingLabel(since: string | undefined): string | null {
  const [now, setNow] = useState<number | null>(null);
  useEffect(() => {
    if (!since) return;
    const tick = () => setNow(Date.now());
    tick();
    const id = setInterval(tick, 30_000);
    return () => clearInterval(id);
  }, [since]);
  if (!since || now == null) return null;
  const startedAt = new Date(since).getTime();
  if (Number.isNaN(startedAt)) return null;
  const secs = Math.max(0, Math.floor((now - startedAt) / 1000));
  if (secs < 60) return `waiting ${secs} s`;
  const mins = Math.floor(secs / 60);
  if (mins < 60) return `waiting ${mins} min`;
  return `waiting ${Math.floor(mins / 60)} h ${mins % 60} min`;
}
