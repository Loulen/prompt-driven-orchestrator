import { useCallback, useEffect, useState } from "react";
import { fetchSettings, updateSettings } from "../api";
import type { InstanceSettings, UpdateSettingsRequest } from "../types";

/**
 * Window bus fired after every write that changes what `GET /settings` serves — the
 * instance form's Save, a staging profile created / renamed / deleted from Settings (#691).
 * On the model of `pdo:agent-profiles-changed`: New Run stays mounted under the Settings
 * overlay and refetches on it, so a profile created in Settings is offered without reload.
 */
export const SETTINGS_CHANGED = "pdo:settings-changed";

export function announceSettingsChanged() {
  window.dispatchEvent(new Event(SETTINGS_CHANGED));
}

/**
 * Instance-wide settings state for the Settings surface (#129, ADR-0015; #690).
 *
 * Mirrors `useLibrary`: a one-shot fetch plus `refresh`/`save`. The fetch is
 * keyed on `open` so reopening the modal re-reads the current values (a knob may
 * have changed via another client, or the daemon may have been restarted).
 */
export function useSettings(open: boolean) {
  const [settings, setSettings] = useState<InstanceSettings | null>(null);
  const [loading, setLoading] = useState(false);
  // #697: true once the initial read has answered (ok or not). The Settings page waits for
  // it before landing on a requested section: the sections above the target mount on that
  // read, and a scroll made before they exist ends up pointing at the wrong place.
  const [settled, setSettled] = useState(false);

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
      .catch(() => {})
      .finally(() => {
        if (!cancelled) setSettled(true);
      });
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

  return { settings, loading, settled, refresh, save };
}
