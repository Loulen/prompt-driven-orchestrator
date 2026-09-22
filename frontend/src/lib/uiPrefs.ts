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

/**
 * #783 — whether a parent row's child runs are expanded when the run list
 * loads (Settings › General › Interface, « Child runs »). Same nature as the
 * theme: per-browser, saved at the change. Absent / unparseable → `true`
 * (« déplié par défaut », CONTEXT.md). Per-row toggles and the expand/collapse-all
 * button are session-only and never write here.
 */
const CHILD_RUNS_KEY = "pdo.ui.childRunsExpanded";

export function loadChildRunsExpanded(): boolean {
  try {
    const raw = localStorage.getItem(CHILD_RUNS_KEY);
    if (raw == null) return true;
    const v: unknown = JSON.parse(raw);
    return typeof v === "boolean" ? v : true;
  } catch {
    return true;
  }
}

export function saveChildRunsExpanded(v: boolean): void {
  try {
    localStorage.setItem(CHILD_RUNS_KEY, JSON.stringify(v));
  } catch {
    // quota / disabled / private mode → in-memory only for this session
  }
}

/**
 * #844 — how the wiring grid shows itself WHILE an edge is being drawn (Settings ›
 * General › Interface, « Wiring grid »). A reading aid, per browser, saved at the
 * change.
 *
 * Note what is NOT a setting: the grid's STEP is a product constant (ADR-0072,
 * `lib/wiringGrid.ts`), because waypoints travel inside the shared pipeline file.
 * Only the feedback — how loudly the lattice announces itself under the cursor —
 * is the reader's business. Absent / unparseable → `dots`.
 */
export type WiringGridFeedback = "none" | "dots" | "lines";

const WIRING_GRID_KEY = "pdo.ui.wiringGridFeedback";

export function loadWiringGridFeedback(): WiringGridFeedback {
  try {
    const raw = localStorage.getItem(WIRING_GRID_KEY);
    if (raw == null) return "dots";
    const v: unknown = JSON.parse(raw);
    return v === "none" || v === "dots" || v === "lines" ? v : "dots";
  } catch {
    return "dots";
  }
}

export function saveWiringGridFeedback(v: WiringGridFeedback): void {
  try {
    localStorage.setItem(WIRING_GRID_KEY, JSON.stringify(v));
  } catch {
    // quota / disabled / private mode → in-memory only for this session
  }
}

/**
 * Stats keeps **no** per-browser preference (#819). The box-plot zoom and the
 * axis mode used to live here under `pdo.stats.*`; they are now ephemeral like
 * every other Stats setting — see `lib/statsFilters.ts`. Stale keys from an
 * older build are left alone: never read, never written, never cleaned up.
 */
