import { useCallback, useEffect, useState } from "react";
import { fetchSettings, updateSettings } from "../api";
import type { InstanceSettings, UpdateSettingsRequest } from "../types";

/**
 * Instance-wide settings state for the SettingsModal (#129, ADR-0015).
 *
 * Mirrors `useLibrary`: a one-shot fetch plus `refresh`/`save`. The fetch is
 * keyed on `open` so reopening the modal re-reads the current values (a knob may
 * have changed via another client, or the daemon may have been restarted).
 */
export function useSettings(open: boolean) {
  const [settings, setSettings] = useState<InstanceSettings | null>(null);
  const [loading, setLoading] = useState(false);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      setSettings(await fetchSettings());
    } catch {
      // ignore — the modal keeps its last-known values
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    fetchSettings()
      .then((data) => {
        if (!cancelled) setSettings(data);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [open]);

  const save = useCallback(async (patch: UpdateSettingsRequest) => {
    // Let the caller catch a rejection (fail-fast 400) and surface it.
    const updated = await updateSettings(patch);
    setSettings(updated);
    return updated;
  }, []);

  return { settings, loading, refresh, save };
}
