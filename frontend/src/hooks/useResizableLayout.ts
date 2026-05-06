import { useCallback, useMemo } from "react";

export const MIN_SIZE_PX = 100;
const MIN_SIZE_PCT = 5;

const STORAGE_KEYS = {
  run: "maestro.layout.run",
  edit: "maestro.layout.edit",
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

export function useResizableLayout(
  mode: "run" | "edit",
  panelIds: string[],
  defaultSizes: Layout,
) {
  const key = STORAGE_KEYS[mode];

  const defaultLayout = useMemo(
    () => loadLayout(key, panelIds, defaultSizes),
    [key, panelIds, defaultSizes],
  );

  const onLayoutChanged = useCallback(
    (layout: Layout) => {
      localStorage.setItem(key, JSON.stringify(layout));
    },
    [key],
  );

  return { defaultLayout, onLayoutChanged, minSizePx: MIN_SIZE_PX };
}
