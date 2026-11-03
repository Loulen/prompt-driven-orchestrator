/**
 * The live Theme and Dark mode (ADR-0093), read from the Apparence of the
 * Actor (`useAppearance`, CONTEXT.md « Apparence »): `useTheme()` for the
 * Settings rows, the top bar's switch and the command field, painted on
 * `<html>` by the Apparence store.
 */
import { initAppearance, updateAppearance, useAppearance } from "./useAppearance";
import type { Mode, Theme, ThemeChoice } from "../lib/theme";

/**
 * Read this browser's copy of the Apparence and paint it. Called once at boot,
 * before the first render: the theme script (`/shared/theme.js`) has already
 * stamped the same values, so there is no flash.
 */
export function initTheme(): ThemeChoice {
  const { theme, mode } = initAppearance();
  return { theme, mode };
}

export function setTheme(theme: Theme): void {
  updateAppearance((appearance) => ({ ...appearance, theme }));
}

export function setMode(mode: Mode): void {
  updateAppearance((appearance) => ({ ...appearance, mode }));
}

export function toggleMode(): void {
  updateAppearance((appearance) => ({ ...appearance, mode: appearance.mode === "dark" ? "light" : "dark" }));
}

export function useTheme(): ThemeChoice & {
  setTheme: (theme: Theme) => void;
  setMode: (mode: Mode) => void;
  toggleMode: () => void;
} {
  const { theme, mode } = useAppearance();
  return { theme, mode, setTheme, setMode, toggleMode };
}
