import { useCallback, useState } from "react";
import type { SettingsPosition, StatsOpenIntent } from "../settings/components/SettingsSurface";

/** Settings and Stats: which is open, and how each was entered. */
export function useSurfaces() {
  // #690: Settings and Stats are full-window surfaces on one shared shell, one open at a
  // time. `openSettings` / `openStats` are the single openers: the gear and the chart icon
  // call them bare, programmatic entries pass a position, which is applied by remounting
  // the surface (`key`). Settings lands where the user last was — its state survives a
  // close on purpose (page-session memory). Stats does the opposite (#819): it forgets
  // its settings on close, and owns that itself (see `StatsModal`).
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [settingsEntry, setSettingsEntry] = useState<{ key: number; position?: SettingsPosition }>(
    { key: 0 },
  );
  const [statsOpen, setStatsOpen] = useState(false);
  const [statsEntry, setStatsEntry] = useState<{ key: number; intent?: StatsOpenIntent }>({
    key: 0,
  });
  const openSettings = useCallback((position?: SettingsPosition) => {
    if (position) setSettingsEntry((entry) => ({ key: entry.key + 1, position }));
    setStatsOpen(false);
    setSettingsOpen(true);
  }, []);
  const openStats = useCallback((intent?: StatsOpenIntent) => {
    // #819 — Stats opens on its defaults every time, so the entry is rebuilt on
    // every open: the intent of a programmatic entry (Settings › Diagnostics →
    // Cost › Pricing details) belongs to the open that carries it and must not
    // survive into the next bare one. The `key` bump covers the case where Stats
    // is already on screen, which no remount would otherwise reach.
    setStatsEntry((entry) => ({ key: entry.key + 1, intent }));
    setSettingsOpen(false);
    setStatsOpen(true);
  }, []);
  return {
    settingsOpen,
    setSettingsOpen,
    settingsEntry,
    statsOpen,
    setStatsOpen,
    statsEntry,
    openSettings,
    openStats,
  };
}
