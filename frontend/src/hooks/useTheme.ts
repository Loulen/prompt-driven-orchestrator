/**
 * The live Theme and Dark mode (ADR-0093): one module store, read by
 * `useTheme()` and painted on `<html>` by `applyAppearance`.
 *
 * Why a store and not per-component state: the appearance is a single property
 * of the document. Its consumers (the Settings rows, the top bar's switch, the
 * command field, the surfaces painted out of CSS) must never disagree.
 *
 * Another tab of the same daemon that changes a preference writes the store;
 * its `storage` event repaints this one too.
 */
import { useSyncExternalStore } from "react";
import {
  applyAppearance,
  DEFAULT_APPEARANCE,
  loadAppearance,
  MODE_STORAGE_KEY,
  saveMode,
  saveTheme,
  THEME_STORAGE_KEY,
  type Appearance,
  type Mode,
  type Theme,
} from "../lib/theme";

let snapshot: Appearance = DEFAULT_APPEARANCE;
const listeners = new Set<() => void>();
let unbindStorage: (() => void) | null = null;

function publish(next: Appearance): void {
  if (next.theme === snapshot.theme && next.mode === snapshot.mode) return;
  snapshot = next;
  applyAppearance(snapshot);
  for (const notify of listeners) notify();
}

/**
 * Read the stored preferences, migrating a value written before 2.68, and
 * paint them. Called once at boot, before the first render: the theme script
 * (`/shared/theme.js`) has already stamped the same values, so there is no
 * flash.
 */
export function initTheme(): Appearance {
  unbindStorage?.();
  unbindStorage = null;
  snapshot = loadAppearance();
  applyAppearance(snapshot);
  for (const notify of listeners) notify();

  if (typeof window !== "undefined") {
    const onStorage = (event: StorageEvent) => {
      if (event.key === THEME_STORAGE_KEY || event.key === MODE_STORAGE_KEY || event.key === null) {
        publish(loadAppearance());
      }
    };
    window.addEventListener("storage", onStorage);
    unbindStorage = () => window.removeEventListener("storage", onStorage);
  }
  return snapshot;
}

function subscribe(notify: () => void): () => void {
  listeners.add(notify);
  return () => listeners.delete(notify);
}

export function setTheme(theme: Theme): void {
  saveTheme(theme);
  publish({ ...snapshot, theme });
}

export function setMode(mode: Mode): void {
  saveMode(mode);
  publish({ ...snapshot, mode });
}

export function toggleMode(): void {
  setMode(snapshot.mode === "dark" ? "light" : "dark");
}

export function useTheme(): Appearance & {
  setTheme: (theme: Theme) => void;
  setMode: (mode: Mode) => void;
  toggleMode: () => void;
} {
  const current = useSyncExternalStore(subscribe, () => snapshot, () => snapshot);
  return { ...current, setTheme, setMode, toggleMode };
}
