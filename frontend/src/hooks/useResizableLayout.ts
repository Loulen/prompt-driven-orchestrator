import { useCallback, useMemo, useRef } from "react";

export const MIN_SIZE_PX = 100;
const MIN_SIZE_PCT = 5;

const STORAGE_KEYS = {
  run: "pdo.layout.run",
  edit: "pdo.layout.edit",
} as const;

export type Layout = Record<string, number>;

export function clampLayout(layout: Layout, minPct: number): Layout {
  const entries = Object.entries(layout);
  const clamped = entries.map(
    ([k, v]) => [k, Math.max(v, minPct)] as const,
  );
  const sum = clamped.reduce((a, [, v]) => a + v, 0);
  if (Math.abs(sum - 100) < 0.01) {
    return Object.fromEntries(clamped);
  }

  const excess = sum - 100;
  const flexible = clamped.filter(([, v]) => v > minPct);
  const flexibleTotal = flexible.reduce((a, [, v]) => a + v, 0);

  if (flexibleTotal <= 0) {
    const scale = 100 / sum;
    return Object.fromEntries(
      clamped.map(([k, v]) => [k, +(v * scale).toFixed(2)]),
    );
  }

  return Object.fromEntries(
    clamped.map(([k, v]) => {
      if (v <= minPct) return [k, v];
      return [k, +(v - excess * (v / flexibleTotal)).toFixed(2)];
    }),
  );
}

function isValidLayout(parsed: unknown, panelIds: string[]): parsed is Layout {
  if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) {
    return false;
  }
  const obj = parsed as Record<string, unknown>;
  return panelIds.every(
    (id) => typeof obj[id] === "number" && (obj[id] as number) >= 0,
  );
}

function loadLayout(
  key: string,
  panelIds: string[],
  defaults: Layout,
): Layout {
  try {
    const raw = localStorage.getItem(key);
    if (!raw) return defaults;
    const parsed: unknown = JSON.parse(raw);
    if (!isValidLayout(parsed, panelIds)) return defaults;
    return clampLayout(parsed, MIN_SIZE_PCT);
  } catch {
    return defaults;
  }
}

/** Below this percentage a collapsible panel counts as collapsed (collapsedSize is 0%). */
const COLLAPSED_PCT = 0.5;

/** A panel the app collapses on its own, and the panel that takes its width meanwhile. */
export interface CollapsiblePanel {
  panelId: string;
  absorbInto: string;
}

/**
 * The layout to persist when `panel` may be collapsed (#949). A collapsed
 * panel's zero width is never written: it keeps its last expanded width from
 * `persisted`, taken back from the panel that absorbed it, so a later expand —
 * or a reload — restores the width the user chose. The other panels' sizes (a
 * drag on another handle while collapsed) still go through. Returns
 * `persisted` unchanged when the absorbing panel is too small to give it back.
 */
export function keepCollapsedWidth(
  next: Layout,
  persisted: Layout,
  panel: CollapsiblePanel,
): Layout {
  const { panelId, absorbInto } = panel;
  if ((next[panelId] ?? 0) > COLLAPSED_PCT) return next;
  const width = persisted[panelId];
  const absorbed = (next[absorbInto] ?? 0) - width;
  if (width === undefined || absorbed < MIN_SIZE_PCT) return persisted;
  return { ...next, [panelId]: width, [absorbInto]: +absorbed.toFixed(2) };
}

export function useResizableLayout(
  mode: "run" | "edit",
  panelIds: string[],
  defaultSizes: Layout,
  collapsible?: CollapsiblePanel,
) {
  const key = STORAGE_KEYS[mode];

  const defaultLayout = useMemo(
    () => loadLayout(key, panelIds, defaultSizes),
    [key, panelIds, defaultSizes],
  );

  // The last persisted layout — the source of a collapsed panel's width.
  const persistedRef = useRef(defaultLayout);
  const collapsibleId = collapsible?.panelId;
  const absorbInto = collapsible?.absorbInto;

  const onLayoutChanged = useCallback(
    (layout: Layout) => {
      const toStore =
        collapsibleId && absorbInto
          ? keepCollapsedWidth(layout, persistedRef.current, {
              panelId: collapsibleId,
              absorbInto,
            })
          : layout;
      persistedRef.current = toStore;
      localStorage.setItem(key, JSON.stringify(toStore));
    },
    [key, collapsibleId, absorbInto],
  );

  /** The last persisted (expanded) size of a panel, in percent. */
  const persistedSize = useCallback(
    (panelId: string): number | undefined => persistedRef.current[panelId],
    [],
  );

  return { defaultLayout, onLayoutChanged, persistedSize, minSizePx: MIN_SIZE_PX };
}
