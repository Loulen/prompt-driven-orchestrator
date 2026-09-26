import { initTheme } from "../hooks/useTheme";

/**
 * Process-wide setup that must run once, before the first render.
 *
 * #759: adopt the stored theme and start following the OS. The inline guard in
 * `index.html` already stamped the same value before first paint; this call
 * takes ownership of it, so a later `prefers-color-scheme` flip is honoured
 * while the preference is `system`.
 */
export function bootstrap(): void {
  initTheme();
}
