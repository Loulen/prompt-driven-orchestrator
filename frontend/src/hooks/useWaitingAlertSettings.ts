/**
 * The live waiting-alert settings (#1011): this browser's two switches and the
 * notification permission, in one module store — the shape of
 * `useColourBlindStatus`. Read by Settings › General › Notifications and by the
 * alert controller (`useWaitingAlerts`).
 *
 * - The `storage` event carries a switch flipped in another tab of the same
 *   browser, with no reload.
 * - The permission is re-read whenever it may have moved behind our back: the
 *   Permissions API's `change`, a return to the tab, a window focus. A user who
 *   unblocks the site in the browser's settings and comes back finds the switch
 *   unlocked on its own.
 * - A notification switch is only ever on while the permission is `granted`:
 *   refused, revoked or reset later, the preference falls back to off.
 */
import { useSyncExternalStore } from "react";
import {
  readNotificationPermission,
  requestNotificationPermission,
} from "../lib/notificationAdapter";
import type { AlertPermission, WaitingAlertPrefs } from "../lib/waitingAlert";
import {
  loadWaitingAlertPrefs,
  saveWaitingAlertPrefs,
  WAITING_ALERT_PREFS_STORAGE_KEY,
} from "../lib/waitingAlertPrefs";

export interface WaitingAlertSettingsState {
  prefs: WaitingAlertPrefs;
  permission: AlertPermission;
  /** The browser's permission prompt is open (the switch waits for its answer). */
  requesting: boolean;
}

let state: WaitingAlertSettingsState = {
  prefs: loadWaitingAlertPrefs(),
  permission: readNotificationPermission(),
  requesting: false,
};
const listeners = new Set<() => void>();
let unbind: (() => void) | null = null;

function publish(next: WaitingAlertSettingsState): void {
  // The notification switch cannot outlive its permission (revoked, reset).
  if (next.prefs.notification && next.permission !== "granted" && !next.requesting) {
    next = { ...next, prefs: { ...next.prefs, notification: false } };
    saveWaitingAlertPrefs(next.prefs);
  }
  if (
    next.prefs.notification === state.prefs.notification &&
    next.prefs.sound === state.prefs.sound &&
    next.permission === state.permission &&
    next.requesting === state.requesting
  ) {
    return;
  }
  state = next;
  for (const notify of listeners) notify();
}

function refreshPermission(): void {
  publish({ ...state, permission: readNotificationPermission() });
}

/**
 * Read the stored switches and the permission, and start following the other
 * tabs and the browser. Called once at boot from `main.tsx`; the hook calls it
 * too on first use, so a test rendering the Settings alone is wired as well.
 */
export function initWaitingAlertSettings(): WaitingAlertSettingsState {
  unbind?.();
  unbind = null;
  publish({ prefs: loadWaitingAlertPrefs(), permission: readNotificationPermission(), requesting: false });

  if (typeof window === "undefined" || !window.addEventListener) return state;
  const onStorage = (e: StorageEvent) => {
    // `key === null`: the whole store was cleared — back to the defaults.
    if (e.key === WAITING_ALERT_PREFS_STORAGE_KEY || e.key === null) {
      publish({ ...state, prefs: loadWaitingAlertPrefs() });
    }
  };
  const onVisibility = () => {
    if (document.visibilityState === "visible") refreshPermission();
  };
  window.addEventListener("storage", onStorage);
  window.addEventListener("focus", refreshPermission);
  document.addEventListener("visibilitychange", onVisibility);

  let permissionStatus: PermissionStatus | null = null;
  let disposed = false;
  try {
    const query = navigator.permissions?.query?.({ name: "notifications" as PermissionName });
    query
      ?.then((status) => {
        if (disposed) return;
        permissionStatus = status;
        status.addEventListener?.("change", refreshPermission);
      })
      .catch(() => {});
  } catch {
    // no Permissions API: focus / visibility re-reads are enough
  }

  unbind = () => {
    disposed = true;
    window.removeEventListener("storage", onStorage);
    window.removeEventListener("focus", refreshPermission);
    document.removeEventListener("visibilitychange", onVisibility);
    permissionStatus?.removeEventListener?.("change", refreshPermission);
  };
  return state;
}

/** Test seam: stop following the browser. */
export function disposeWaitingAlertSettings(): void {
  unbind?.();
  unbind = null;
}

export function getWaitingAlertSettings(): WaitingAlertSettingsState {
  return state;
}

function setPrefs(prefs: WaitingAlertPrefs): void {
  saveWaitingAlertPrefs(prefs);
  publish({ ...state, prefs });
}

/**
 * Flip « Browser notification ». Turning it on with the permission still
 * `default` asks the browser first: granted keeps it on; refused (or the
 * prompt dismissed) leaves it off — `denied` then greys the switch.
 */
export async function setNotificationEnabled(on: boolean): Promise<void> {
  if (!on) {
    setPrefs({ ...state.prefs, notification: false });
    return;
  }
  if (state.requesting) return;
  let permission = readNotificationPermission();
  if (permission === "default") {
    publish({ ...state, permission, requesting: true });
    permission = await requestNotificationPermission();
    publish({ ...state, permission, requesting: false });
  }
  if (permission !== "granted") {
    publish({ ...state, permission });
    return;
  }
  setPrefs({ ...state.prefs, notification: true });
}

/** Flip « Sound » (#1012 renders the switch; no permission involved). */
export function setSoundEnabled(on: boolean): void {
  setPrefs({ ...state.prefs, sound: on });
}

function subscribe(notify: () => void): () => void {
  if (!unbind) initWaitingAlertSettings();
  listeners.add(notify);
  return () => listeners.delete(notify);
}

export function useWaitingAlertSettings(): WaitingAlertSettingsState & {
  setNotificationEnabled: (on: boolean) => Promise<void>;
  setSoundEnabled: (on: boolean) => void;
} {
  const current = useSyncExternalStore(subscribe, () => state, () => state);
  return { ...current, setNotificationEnabled, setSoundEnabled };
}
