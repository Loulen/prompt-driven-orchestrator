/**
 * The UI scale (Settings › Interface, « UI scale »): 100, 110 or 120 % of the
 * dense sizes, 120 by default. A presentation preference, per browser, like the
 * theme (`theme.ts`): stored under `pdo.*`, read and written behind a guard so
 * a disabled store degrades to the default for the session.
 *
 * It is painted as `--app-zoom` on `<html>`; `#root` applies it with CSS `zoom`
 * (index.css). The inline script of `index.html` stamps the same value before
 * the first paint; `noFlashTheme.test.ts` checks the two agree.
 */
import { useSyncExternalStore } from "react";

export const UI_SCALE_STORAGE_KEY = "pdo.ui.scale";

export type UiScale = 100 | 110 | 120;

export const UI_SCALES: readonly UiScale[] = [100, 110, 120];

export const DEFAULT_UI_SCALE: UiScale = 120;

function isUiScale(v: unknown): v is UiScale {
  return typeof v === "number" && (UI_SCALES as readonly number[]).includes(v);
}

/** Stored scale. Absent / unknown / unreadable → 120. */
export function loadUiScale(): UiScale {
  try {
    const raw = Number(localStorage.getItem(UI_SCALE_STORAGE_KEY));
    return isUiScale(raw) ? raw : DEFAULT_UI_SCALE;
  } catch {
    return DEFAULT_UI_SCALE;
  }
}

function saveUiScale(scale: UiScale): void {
  try {
    localStorage.setItem(UI_SCALE_STORAGE_KEY, String(scale));
  } catch {
    // quota / disabled / private mode → in-memory only for this session
  }
}

/** Paint `scale` on `<html>`. */
export function applyUiScale(scale: UiScale, root: HTMLElement = document.documentElement): void {
  root.style.setProperty("--app-zoom", String(scale / 100));
}

let current: UiScale = DEFAULT_UI_SCALE;
const listeners = new Set<() => void>();

/** Read the stored scale and paint it. Called once at boot. */
export function initUiScale(): UiScale {
  current = loadUiScale();
  applyUiScale(current);
  return current;
}

/** Choose `scale`: saved at once, painted at once. */
export function setUiScale(scale: UiScale): void {
  if (scale === current) return;
  current = scale;
  saveUiScale(scale);
  applyUiScale(scale);
  for (const notify of listeners) notify();
}

/** The live scale as a zoom factor (1, 1.1, 1.2), outside React. */
export function uiZoom(): number {
  return current / 100;
}

function subscribe(notify: () => void): () => void {
  listeners.add(notify);
  return () => listeners.delete(notify);
}

/** The live scale, in percent: re-renders on a change. */
export function useUiScale(): UiScale {
  return useSyncExternalStore(subscribe, () => current, () => DEFAULT_UI_SCALE);
}
