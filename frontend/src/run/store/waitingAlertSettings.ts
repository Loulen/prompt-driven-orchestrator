/**
 * The live settings of the Alerte d'attente: this browser's preferences
 * (`run/model/alertScope.ts`) and the notification permission, in one module
 * store. Read by Settings › Notifications and by the alert controller.
 *
 * - The preferences live in this browser's `localStorage` (`pdo.*`), like the
 *   UI scale: never `PUT /settings`, never the Apparence. A switch flipped in
 *   another tab of the browser arrives through the `storage` event.
 * - The permission is read again whenever it may have moved behind our back:
 *   the Permissions API's `change`, a return to the tab, a window focus. A
 *   user who unblocks the site and comes back finds the switch unlocked.
 * - The notification switch is only ever on while the permission is
 *   `granted`: refused, revoked or reset later, it falls back to off.
 */
import { useSyncExternalStore } from "react";
import { DEFAULT_WAITING_ALERT_PREFS, normalizePrefs, type WaitingAlertPrefs } from "../model/alertScope";
import type { AlertPermission } from "../model/waitingAlert";
import { readNotificationPermission, requestNotificationPermission } from "./notificationAdapter";

export const WAITING_ALERT_PREFS_STORAGE_KEY = "pdo.ui.waitingAlerts";

export function loadWaitingAlertPrefs(): WaitingAlertPrefs {
  try {
    const raw = localStorage.getItem(WAITING_ALERT_PREFS_STORAGE_KEY);
    return raw ? normalizePrefs(JSON.parse(raw)) : { ...DEFAULT_WAITING_ALERT_PREFS };
  } catch {
    return { ...DEFAULT_WAITING_ALERT_PREFS };
  }
}

function saveWaitingAlertPrefs(prefs: WaitingAlertPrefs): void {
  try {
    localStorage.setItem(WAITING_ALERT_PREFS_STORAGE_KEY, JSON.stringify(prefs));
  } catch {
    // quota / disabled / private mode: in memory for this session
  }
}

export interface WaitingAlertSettingsState {
  prefs: WaitingAlertPrefs;
  permission: AlertPermission;
  /** The browser's permission prompt is open: the switch waits for its answer. */
  requesting: boolean;
}

let state: WaitingAlertSettingsState = {
  prefs: loadWaitingAlertPrefs(),
  permission: readNotificationPermission(),
  requesting: false,
};
const listeners = new Set<() => void>();
let unbind: (() => void) | null = null;

function samePrefs(a: WaitingAlertPrefs, b: WaitingAlertPrefs): boolean {
  return (
    a.notification === b.notification &&
    a.sound === b.sound &&
    a.scope === b.scope &&
    a.projects.length === b.projects.length &&
    a.projects.every((id, i) => id === b.projects[i])
  );
}

function publish(next: WaitingAlertSettingsState): void {
  // The notification switch cannot outlive its permission.
  if (next.prefs.notification && next.permission !== "granted" && !next.requesting) {
    next = { ...next, prefs: { ...next.prefs, notification: false } };
    saveWaitingAlertPrefs(next.prefs);
  }
  if (samePrefs(next.prefs, state.prefs) && next.permission === state.permission && next.requesting === state.requesting) {
    return;
  }
  state = next;
  for (const notify of listeners) notify();
}

function refreshPermission(): void {
  publish({ ...state, permission: readNotificationPermission() });
}

/**
 * Read the stored preferences and the permission, and follow the other tabs
 * and the browser. Called at boot; the hook calls it on first use too, so a
 * test rendering the Settings alone is wired as well.
 */
export function initWaitingAlertSettings(): WaitingAlertSettingsState {
  unbind?.();
  unbind = null;
  publish({ prefs: loadWaitingAlertPrefs(), permission: readNotificationPermission(), requesting: false });
  if (typeof window === "undefined" || !window.addEventListener) return state;

  const onStorage = (e: StorageEvent) => {
    // `key === null`: the whole store was cleared, back to the defaults.
    // The permission is read anew too: the tab that wrote may have just been
    // granted it, and a stale one here would turn its switch back off.
    if (e.key === WAITING_ALERT_PREFS_STORAGE_KEY || e.key === null) {
      publish({ ...state, prefs: loadWaitingAlertPrefs(), permission: readNotificationPermission() });
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
    navigator.permissions
      ?.query?.({ name: "notifications" as PermissionName })
      ?.then((status) => {
        if (disposed) return;
        permissionStatus = status;
        status.addEventListener?.("change", refreshPermission);
      })
      .catch(() => {});
  } catch {
    // no Permissions API: the focus and visibility reads are enough
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

/** Is an alert switch on in this browser? */
export function waitingAlertsOn(): boolean {
  return state.prefs.notification || state.prefs.sound;
}

function setPrefs(prefs: WaitingAlertPrefs): void {
  saveWaitingAlertPrefs(prefs);
  publish({ ...state, prefs });
}

/**
 * Flip « Browser notification ». Turning it on while the permission is still
 * `default` asks the browser first: granted keeps it on; refused (or the
 * prompt dismissed) leaves it off, and `denied` then greys the switch.
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

/** Flip « Sound »: no permission involved. */
export function setSoundEnabled(on: boolean): void {
  setPrefs({ ...state.prefs, sound: on });
}

/** Change the Portée d'alerte. */
export function setAlertScope(scope: WaitingAlertPrefs["scope"]): void {
  setPrefs({ ...state.prefs, scope });
}

/** Check or uncheck one Projet of the `projects` scope. */
export function setAlertProject(projectId: string, checked: boolean): void {
  const others = state.prefs.projects.filter((id) => id !== projectId);
  setPrefs({ ...state.prefs, projects: checked ? [...others, projectId] : others });
}

function subscribe(notify: () => void): () => void {
  if (!unbind) initWaitingAlertSettings();
  listeners.add(notify);
  return () => listeners.delete(notify);
}

/** The settings, live. */
export function useWaitingAlertSettings(): WaitingAlertSettingsState {
  return useSyncExternalStore(subscribe, () => state, () => state);
}
