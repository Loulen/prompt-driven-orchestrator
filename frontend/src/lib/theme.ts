/**
 * Theme preference (#759) — light / dark / system.
 *
 * A *presentation* preference, so it lives in the `pdo.*` localStorage namespace
 * next to `uiPrefs` and `dismissedBanners`, NOT in `instance_config`: ADR-0015
 * covers daemon-wide runtime knobs, and two browsers pointed at the same daemon
 * would fight over one stored row. Same discipline as `uiPrefs`: guard BOTH read
 * and write so private mode / a disabled or full store degrades to an in-memory
 * default for the session instead of throwing.
 *
 * `dark` is the default (#781): PDO has always had the dark look, so a client
 * with no stored preference paints dark whatever the OS asks — a light-mode OS
 * must not silently restyle the app on first paint. `system` stays available
 * in Settings for whoever wants to follow the OS; an explicit `light` / `dark`
 * pins it.
 */

export const THEME_STORAGE_KEY = "pdo.ui.theme";

/** What the person chose. `system` defers to `prefers-color-scheme`. */
export type ThemePreference = "light" | "dark" | "system";

/** What is actually painted. `system` has been resolved away. */
export type ResolvedTheme = "light" | "dark";

const PREFERENCES: readonly ThemePreference[] = ["light", "dark", "system"];

function isThemePreference(v: unknown): v is ThemePreference {
  return typeof v === "string" && (PREFERENCES as readonly string[]).includes(v);
}

/** Stored preference. Absent / unknown / unreadable → `dark` (#781). */
export function loadThemePreference(): ThemePreference {
  try {
    const raw = localStorage.getItem(THEME_STORAGE_KEY);
    return isThemePreference(raw) ? raw : "dark";
  } catch {
    return "dark";
  }
}

export function saveThemePreference(pref: ThemePreference): void {
  try {
    localStorage.setItem(THEME_STORAGE_KEY, pref);
  } catch {
    // quota / disabled / private mode → in-memory only for this session
  }
}

/** Whether the OS asks for a dark presentation. No `matchMedia` (old engine,
 *  non-browser host) → dark, which is the look PDO has always had. */
export function systemPrefersDark(): boolean {
  const mm = typeof matchMedia === "function" ? matchMedia : undefined;
  if (!mm) return true;
  try {
    return mm("(prefers-color-scheme: dark)").matches;
  } catch {
    return true;
  }
}

/** The theme actually painted, once `system` has been resolved against the OS. */
export function resolveTheme(pref: ThemePreference, systemDark: boolean): ResolvedTheme {
  if (pref === "system") return systemDark ? "dark" : "light";
  return pref;
}

/**
 * Paint the theme: `data-theme` drives the palette in `index.css`, `color-scheme`
 * makes the engine's own chrome (scrollbars, form widgets, canvas default) follow.
 *
 * The attribute always carries a RESOLVED value — `system` never reaches the DOM,
 * so the stylesheet needs no `prefers-color-scheme` media query and the two paths
 * (stored choice, OS default) can never disagree.
 */
export function applyResolvedTheme(
  theme: ResolvedTheme,
  root: HTMLElement = document.documentElement,
): void {
  root.setAttribute("data-theme", theme);
  root.style.colorScheme = theme;
}

/**
 * The theme currently painted, read straight off `<html>`.
 *
 * For the handful of helpers that resolve a colour outside React and cannot
 * take the theme as an argument (see `harnessColor`). Everything inside the
 * component tree should use `useTheme()`, which also re-renders on a switch.
 */
export function currentTheme(root: HTMLElement = document.documentElement): ResolvedTheme {
  return root.getAttribute("data-theme") === "light" ? "light" : "dark";
}
