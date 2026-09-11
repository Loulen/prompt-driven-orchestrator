/**
 * The live theme (#759): one module store, read by `useTheme()` and painted on
 * `<html>` by `applyResolvedTheme`.
 *
 * Why a store and not per-component state: the resolved theme is a single
 * property of the document. Two mounted consumers (the Settings selector, any
 * future quick toggle) must never disagree, and the OS listener must be bound
 * once, not per consumer.
 *
 * The OS listener stays bound whatever the preference is, and the snapshot is
 * recomputed through `resolveTheme`. A pinned `light` / `dark` therefore absorbs
 * the OS change without a special case: the resolved value simply does not move.
 */
import { useSyncExternalStore } from "react";
import {
  applyResolvedTheme,
  loadThemePreference,
  resolveTheme,
  saveThemePreference,
  systemPrefersDark,
  type ResolvedTheme,
  type ThemePreference,
} from "../lib/theme";

export interface ThemeSnapshot {
  preference: ThemePreference;
  resolved: ResolvedTheme;
}

let snapshot: ThemeSnapshot = { preference: "system", resolved: "dark" };
const listeners = new Set<() => void>();
let unbindOs: (() => void) | null = null;

function publish(next: ThemeSnapshot): void {
  if (next.preference === snapshot.preference && next.resolved === snapshot.resolved) return;
  snapshot = next;
  applyResolvedTheme(snapshot.resolved);
  for (const notify of listeners) notify();
}

function recompute(preference: ThemePreference): void {
  publish({ preference, resolved: resolveTheme(preference, systemPrefersDark()) });
}

/**
 * Read the stored preference, paint it, and start following the OS. Called once
 * at boot from `main.tsx`, before the first render — the inline script in
 * `index.html` has already stamped the same value, so this re-run is a no-op for
 * the eye and there is no flash.
 */
export function initTheme(): ResolvedTheme {
  unbindOs?.();
  unbindOs = null;

  const preference = loadThemePreference();
  snapshot = { preference, resolved: resolveTheme(preference, systemPrefersDark()) };
  applyResolvedTheme(snapshot.resolved);
  for (const notify of listeners) notify();

  const mq =
    typeof matchMedia === "function" ? matchMedia("(prefers-color-scheme: dark)") : undefined;
  if (mq?.addEventListener) {
    const onOsChange = () => recompute(snapshot.preference);
    mq.addEventListener("change", onOsChange);
    unbindOs = () => mq.removeEventListener("change", onOsChange);
  }
  return snapshot.resolved;
}

function subscribe(notify: () => void): () => void {
  listeners.add(notify);
  return () => listeners.delete(notify);
}

export function setThemePreference(preference: ThemePreference): void {
  saveThemePreference(preference);
  recompute(preference);
}

export function useTheme(): ThemeSnapshot & {
  setPreference: (preference: ThemePreference) => void;
} {
  const current = useSyncExternalStore(subscribe, () => snapshot, () => snapshot);
  return { ...current, setPreference: setThemePreference };
}
