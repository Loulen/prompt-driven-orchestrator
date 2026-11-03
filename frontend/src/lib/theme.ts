/**
 * The Theme and the Dark mode (ADR-0093): two preferences of this browser,
 * independent of each other.
 *
 *  - The **Theme** chooses the colours: `acogni` (the default) or `classic`.
 *  - The **Dark mode** is a switch, on by default: `mode` is `dark` or `light`.
 *
 * Presentation preferences, so they live in the `pdo.*` localStorage namespace
 * next to `uiPrefs`, NOT in `instance_config`: two browsers pointed at the same
 * daemon would fight over one stored row. Both read and write are guarded, so
 * private mode or a full store degrades to the defaults for the session.
 *
 * Before 2.68 `pdo.ui.theme` held the luminosity (`light`, `dark` or `system`).
 * Such a value is migrated once: `light` and `dark` become the mode, `system`
 * the mode the operating system asks for at the migration; the Theme is
 * Acogni. Any other unknown value is rewritten as the default. The shared
 * theme script (`public/shared/theme.js`) applies the same rule before the
 * first paint, on every page of the daemon; `noFlashTheme.test.ts` holds the
 * two together.
 */

export const THEME_STORAGE_KEY = "pdo.ui.theme";
export const MODE_STORAGE_KEY = "pdo.ui.mode";

export type Theme = "acogni" | "classic";
export type Mode = "dark" | "light";

export interface ThemeChoice {
  theme: Theme;
  mode: Mode;
}

export const THEMES: readonly Theme[] = ["acogni", "classic"];
export const DEFAULT_THEME_CHOICE: ThemeChoice = { theme: "acogni", mode: "dark" };

const LEGACY = ["light", "dark", "system"];

function isTheme(v: unknown): v is Theme {
  return typeof v === "string" && (THEMES as readonly string[]).includes(v);
}

function read(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function write(key: string, value: string): void {
  try {
    localStorage.setItem(key, value);
  } catch {
    // quota / disabled / private mode → in-memory only for this session
  }
}

/** Whether the OS asks for a dark presentation. No `matchMedia` (old engine,
 *  non-browser host) → dark, the default mode. */
export function systemPrefersDark(): boolean {
  const mm = typeof matchMedia === "function" ? matchMedia : undefined;
  if (!mm) return true;
  try {
    return mm("(prefers-color-scheme: dark)").matches;
  } catch {
    return true;
  }
}

/**
 * The stored Theme and mode; an absent, unknown or unreadable value gives the
 * default, and an unknown one is rewritten as it. A value written before 2.68
 * is migrated and the result stored, so a `system` resolves once, against the
 * OS of the moment (`systemDark`).
 */
export function loadThemeChoice(systemDark: boolean = systemPrefersDark()): ThemeChoice {
  const stored = read(THEME_STORAGE_KEY);
  if (stored !== null && LEGACY.includes(stored)) {
    const mode: Mode = stored === "system" ? (systemDark ? "dark" : "light") : (stored as Mode);
    write(THEME_STORAGE_KEY, DEFAULT_THEME_CHOICE.theme);
    write(MODE_STORAGE_KEY, mode);
    return { theme: DEFAULT_THEME_CHOICE.theme, mode };
  }
  const storedMode = read(MODE_STORAGE_KEY);
  const theme = isTheme(stored) ? stored : DEFAULT_THEME_CHOICE.theme;
  const mode = storedMode === "light" || storedMode === "dark" ? storedMode : DEFAULT_THEME_CHOICE.mode;
  if (stored !== null && stored !== theme) write(THEME_STORAGE_KEY, theme);
  if (storedMode !== null && storedMode !== mode) write(MODE_STORAGE_KEY, mode);
  return { theme, mode };
}

export function saveTheme(theme: Theme): void {
  write(THEME_STORAGE_KEY, theme);
}

export function saveMode(mode: Mode): void {
  write(MODE_STORAGE_KEY, mode);
}

/**
 * Paint the appearance on `<html>`: `data-theme` and `data-mode` select the
 * palette of `/shared/tokens.css`, `color-scheme` makes the engine's own chrome
 * (scrollbars, form widgets) follow the mode.
 */
export function applyThemeChoice(
  { theme, mode }: ThemeChoice,
  root: HTMLElement = document.documentElement,
): void {
  root.setAttribute("data-theme", theme);
  root.setAttribute("data-mode", mode);
  root.style.colorScheme = mode;
}

/**
 * The mode painted right now, read off `<html>`.
 *
 * For the helpers that resolve a colour outside React and cannot take the mode
 * as an argument (see `harnessColor`). Everything inside the component tree
 * uses `useTheme()`, which also re-renders on a switch.
 */
export function currentMode(root: HTMLElement = document.documentElement): Mode {
  return root.getAttribute("data-mode") === "light" ? "light" : "dark";
}
