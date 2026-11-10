import { useCallback, useEffect, useRef, useState } from "react";
import { closePage, leaveSettingsPage, usePages, type OpenPage } from "../shared/extensions/deepLink";
import type { SettingsPosition, StatsOpenIntent } from "../settings/components/SettingsSurface";

/** Whether `page` is a page of the top bar (`topbar/start`, `topbar/end`): a surface of its own. */
function isTopbarPage(page: OpenPage | null): boolean {
  return page?.location === "topbar/start" || page?.location === "topbar/end";
}

/** Close the page of the top bar on screen, if one is. */
function leaveTopbarPage(): void {
  if (isTopbarPage(usePages.getState().open)) closePage();
}

/**
 * The surfaces under the top bar (CORE-183): Settings, Stats and the pages of the top bar
 * (`ExtensionPageHost`), one open at a time. Opening one replaces the one on screen; the
 * icon of the open one toggles it shut.
 */
export function useSurfaces() {
  // #690: Settings and Stats are full-window surfaces on one shared shell, one open at a
  // time. `openSettings` / `openStats` are the single openers: the gear and the chart icon
  // call them bare, programmatic entries pass a position, which is applied by remounting
  // the surface (`key`). Settings lands where the user last was — its state survives a
  // close on purpose (page-session memory). Stats does the opposite (#819): it forgets
  // its settings on close, and owns that itself (see `StatsModal`).
  const [settingsOpen, setSettingsOpenState] = useState(false);
  // Read by the switches, which may run before a render (a page opened from the store).
  const settingsOpenRef = useRef(false);
  const setSettingsOpen = useCallback((open: boolean) => {
    settingsOpenRef.current = open;
    setSettingsOpenState(open);
  }, []);
  const [settingsEntry, setSettingsEntry] = useState<{ key: number; position?: SettingsPosition }>(
    { key: 0 },
  );
  const [statsOpen, setStatsOpen] = useState(false);
  const [statsEntry, setStatsEntry] = useState<{ key: number; intent?: StatsOpenIntent }>({
    key: 0,
  });
  // Settings closes itself, behind its dirty-draft guard (CORE-183): a close asked from
  // outside bumps this, and the surface runs its own close. What waits for that close (the
  // surface switched to) runs once Settings is gone; « Keep editing » cancels it instead.
  const [settingsCloseRequest, setSettingsCloseRequest] = useState(0);
  const pending = useRef<{ then: () => void; cancel?: () => void } | null>(null);
  const leaveSettings = useCallback((then: () => void, cancel?: () => void) => {
    if (!settingsOpenRef.current) {
      then();
      return;
    }
    pending.current = { then, cancel };
    setSettingsCloseRequest((n) => n + 1);
  }, []);
  /** Settings is gone (its ✕, Escape, or a close asked from outside): what waited runs. */
  const settingsClosed = useCallback(() => {
    setSettingsOpen(false);
    const waiting = pending.current;
    pending.current = null;
    waiting?.then();
  }, [setSettingsOpen]);
  /** The user kept editing: Settings stays, the switch that waited is undone. */
  const settingsCloseCancelled = useCallback(() => {
    const waiting = pending.current;
    pending.current = null;
    waiting?.cancel?.();
  }, []);
  const openSettings = useCallback(
    (position?: SettingsPosition) => {
      if (position) {
        // A position names a category of the core: it leaves an extension's page.
        leaveSettingsPage();
        setSettingsEntry((entry) => ({ key: entry.key + 1, position }));
      }
      leaveTopbarPage();
      setStatsOpen(false);
      setSettingsOpen(true);
    },
    [setSettingsOpen],
  );
  const openStats = useCallback(
    (intent?: StatsOpenIntent) => {
      // From Settings, through its guard: Stats opens once Settings is gone.
      leaveSettings(() => {
        // #819 — Stats opens on its defaults every time, so the entry is rebuilt on
        // every open: the intent of a programmatic entry (Settings › Diagnostics →
        // Cost › Pricing details) belongs to the open that carries it and must not
        // survive into the next bare one. The `key` bump covers the case where Stats
        // is already on screen, which no remount would otherwise reach.
        setStatsEntry((entry) => ({ key: entry.key + 1, intent }));
        leaveTopbarPage();
        setStatsOpen(true);
      });
    },
    [leaveSettings],
  );
  /** Settings' icon: opens Settings, or, open, asks it to close. */
  const toggleSettings = useCallback(() => {
    if (settingsOpen) leaveSettings(() => {});
    else openSettings();
  }, [settingsOpen, leaveSettings, openSettings]);
  /** Stats' icon: opens Stats, or closes it. */
  const toggleStats = useCallback(() => {
    if (statsOpen) setStatsOpen(false);
    else openStats();
  }, [statsOpen, openStats]);
  /** Back to the Workbench: every surface closes (Settings through its guard), then `then`. */
  const closeSurfaces = useCallback(
    (then?: () => void) => {
      leaveTopbarPage();
      setStatsOpen(false);
      leaveSettings(() => then?.());
    },
    [leaveSettings],
  );
  // A page of `settings/page` opened by its deep link (or by `openPage`): the
  // Settings surface shows it, so it opens; the surface picks the page itself.
  // A page of the top bar replaces Settings (through its guard: kept, the page closes)
  // and Stats.
  useEffect(() => {
    const show = (page: OpenPage | null) => {
      if (isTopbarPage(page)) {
        setStatsOpen(false);
        leaveSettings(
          () => {},
          () => {
            if (usePages.getState().open === page) closePage();
          },
        );
        return;
      }
      if (page?.location !== "settings/page") return;
      setStatsOpen(false);
      setSettingsOpen(true);
    };
    show(usePages.getState().open);
    return usePages.subscribe((state, previous) => {
      if (state.open !== previous.open) show(state.open);
    });
  }, [leaveSettings, setSettingsOpen]);
  // The top bar's Settings and Stats icons: toggles, selected while their surface is open.
  const topBar = {
    onOpenSettings: toggleSettings,
    onOpenStats: toggleStats,
    settingsOpen,
    statsOpen,
  };
  return {
    topBar,
    settingsOpen,
    setSettingsOpen,
    settingsEntry,
    statsOpen,
    setStatsOpen,
    statsEntry,
    openSettings,
    openStats,
    toggleSettings,
    toggleStats,
    settingsCloseRequest,
    settingsClosed,
    settingsCloseCancelled,
    closeSurfaces,
  };
}
