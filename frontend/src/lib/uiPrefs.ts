/**
 * Per-client UI presentation preferences (#342).
 *
 * Deliberately OUTSIDE `instance_config`: ADR-0015 covers only daemon-wide
 * runtime knobs (stored → env → default), where a value has a meaningful env
 * tier and a single daemon-wide scope. A local presentation preference has
 * neither — two browsers pointed at the same daemon would fight over one
 * stored row. Same discipline as `useResizableLayout` / `dismissedBanners`
 * (the `pdo.*` localStorage namespace): guard BOTH read and write so private
 * mode / a disabled or full store degrades to an in-memory default for the
 * session instead of throwing.
 */
const KEY = "pdo.ui.tabsDisabled";

/** Whether single-tab mode is enabled. Absent / unparseable → `false` (the
 *  default is multi-tab, so a fresh client accumulates tabs). */
export function loadTabsDisabled(): boolean {
  try {
    const raw = localStorage.getItem(KEY);
    if (raw == null) return false;
    const v: unknown = JSON.parse(raw);
    return typeof v === "boolean" ? v : false;
  } catch {
    return false;
  }
}

export function saveTabsDisabled(v: boolean): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(v));
  } catch {
    // quota / disabled / private mode → in-memory only for this session
  }
}
