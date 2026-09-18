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
 * #811 — Stats › Performance box-plot reading preferences, « par navigateur,
 * comme le thème » (CONTEXT.md, « Niveau de zoom d'un box-plot »). Two keys
 * under the `pdo.stats.` namespace rather than one object: they are set from
 * two independent controls, and a corrupt half should not cost the other one.
 *
 * Stored as the bare word, not JSON: the value IS the vocabulary
 * (`full`/`fenced`/`box`, `shared`/`independent`), and a reader poking at
 * localStorage should see it as such.
 */
export type StatsZoom = "full" | "fenced" | "box";
export type StatsAxis = "shared" | "independent";

const ZOOM_KEY = "pdo.stats.zoom";
const AXIS_KEY = "pdo.stats.axis";

const ZOOMS: readonly StatsZoom[] = ["full", "fenced", "box"];
const AXES: readonly StatsAxis[] = ["shared", "independent"];

/** Absent / unknown word → `"full"`: min–max whiskers, today's picture, so a
 *  fresh browser never opens on a cropped plot it did not ask for. */
export function loadStatsZoom(): StatsZoom {
  try {
    const raw = localStorage.getItem(ZOOM_KEY);
    return ZOOMS.find((zoom) => zoom === raw) ?? "full";
  } catch {
    return "full";
  }
}

export function saveStatsZoom(zoom: StatsZoom): void {
  try {
    localStorage.setItem(ZOOM_KEY, zoom);
  } catch {
    // quota / disabled / private mode → in-memory only for this session
  }
}

/** Absent / unknown word → `"shared"`: one axis per metric, so rows stay
 *  comparable until the reader asks otherwise. */
export function loadStatsAxis(): StatsAxis {
  try {
    const raw = localStorage.getItem(AXIS_KEY);
    return AXES.find((axis) => axis === raw) ?? "shared";
  } catch {
    return "shared";
  }
}

export function saveStatsAxis(axis: StatsAxis): void {
  try {
    localStorage.setItem(AXIS_KEY, axis);
  } catch {
    // quota / disabled / private mode → in-memory only for this session
  }
}
