import { useCallback, useEffect, useState } from "react";
import type { TelemetryStatus } from "../../shared/generated/TelemetryStatus";
import { fetchTelemetryStatus, sendTestExport } from "../api/telemetry";
import { SETTINGS_CHANGED } from "./useSettings";

/** How often the page re-reads the export's status while it is shown. */
const POLL_MS = 5000;
/** A Save reloads the export detached: read the status again shortly after. */
const AFTER_SAVE_MS = 400;

/**
 * The OTLP export's status for Settings › Telemetry (ADR-0090): read while the page is
 * shown, again after every settings write, and after a test export. A clock ticks each
 * second so « Last export 12 s ago » stays true.
 */
export function useTelemetryStatus(active: boolean) {
  const [status, setStatus] = useState<TelemetryStatus | null>(null);
  const [testing, setTesting] = useState(false);
  const [testError, setTestError] = useState<string | null>(null);
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    if (!active) return;
    let cancelled = false;
    const refresh = () =>
      fetchTelemetryStatus()
        .then((next) => {
          if (!cancelled) setStatus(next);
        })
        // keep the last-known status
        .catch(() => {});
    void refresh();
    const poll = setInterval(() => void refresh(), POLL_MS);
    const clock = setInterval(() => setNow(Date.now()), 1000);
    let pending: ReturnType<typeof setTimeout> | null = null;
    const onChanged = () => {
      if (pending) clearTimeout(pending);
      pending = setTimeout(() => void refresh(), AFTER_SAVE_MS);
    };
    window.addEventListener(SETTINGS_CHANGED, onChanged);
    return () => {
      cancelled = true;
      clearInterval(poll);
      clearInterval(clock);
      if (pending) clearTimeout(pending);
      window.removeEventListener(SETTINGS_CHANGED, onChanged);
    };
  }, [active]);

  const testExport = useCallback(async () => {
    setTesting(true);
    setTestError(null);
    try {
      setStatus(await sendTestExport());
    } catch (error) {
      setTestError(error instanceof Error ? error.message : String(error));
    } finally {
      setTesting(false);
      setNow(Date.now());
    }
  }, []);

  return { status, testing, testError, testExport, now };
}

/** « 12 s ago », « 3 min ago », « 2 h ago ». */
export function ago(at: string, now: number): string {
  const seconds = Math.max(0, Math.round((now - Date.parse(at)) / 1000));
  if (seconds < 60) return `${seconds} s ago`;
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes} min ago`;
  return `${Math.round(minutes / 60)} h ago`;
}
