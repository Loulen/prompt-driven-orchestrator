/**
 * The live « Colour-blind friendly status colours » option (#1005): one module
 * store, read by `useColourBlindStatus()` and painted on `<html>` by
 * `applyColourBlindStatus` — the same shape as `useTheme`.
 *
 * The `storage` listener is what makes it reach the other open tabs: a switch in
 * one tab writes localStorage, every other tab of the same origin receives the
 * event and repaints, with no reload and no Save.
 */
import { useSyncExternalStore } from "react";
import {
  applyColourBlindStatus,
  COLOUR_BLIND_STATUS_STORAGE_KEY,
  loadColourBlindStatus,
  saveColourBlindStatus,
} from "../lib/statusPalette";

let enabled = false;
const listeners = new Set<() => void>();
let unbindStorage: (() => void) | null = null;

function publish(next: boolean): void {
  applyColourBlindStatus(next);
  if (next === enabled) return;
  enabled = next;
  for (const notify of listeners) notify();
}

/**
 * Read the stored preference, paint it, and start following the other tabs.
 * Called once at boot from `main.tsx` — the inline guard of `index.html` has
 * already stamped the same value, so this is a no-op for the eye.
 */
export function initColourBlindStatus(): boolean {
  unbindStorage?.();
  unbindStorage = null;

  publish(loadColourBlindStatus());

  if (typeof window !== "undefined" && window.addEventListener) {
    // `key === null`: the whole store was cleared — back to the stored (default) value.
    const onStorage = (e: StorageEvent) => {
      if (e.key === COLOUR_BLIND_STATUS_STORAGE_KEY || e.key === null) {
        publish(loadColourBlindStatus());
      }
    };
    window.addEventListener("storage", onStorage);
    unbindStorage = () => window.removeEventListener("storage", onStorage);
  }
  return enabled;
}

function subscribe(notify: () => void): () => void {
  listeners.add(notify);
  return () => listeners.delete(notify);
}

export function setColourBlindStatus(on: boolean): void {
  saveColourBlindStatus(on);
  publish(on);
}

export function useColourBlindStatus(): { enabled: boolean; setEnabled: (on: boolean) => void } {
  const current = useSyncExternalStore(subscribe, () => enabled, () => enabled);
  return { enabled: current, setEnabled: setColourBlindStatus };
}
