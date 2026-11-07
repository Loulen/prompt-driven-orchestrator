import { useCallback, useEffect, useRef, useState } from "react";
import { fetchHarnessCatalog, refreshHarnessModels } from "../api/catalog";
import { useRefusal } from "../../shared/authorization/useRefusals";
import { harnessCatalog, type HarnessCatalog } from "../model/catalog";

/**
 * Window bus fired when a harness's model catalogue was probed again: here, or
 * in another browser (the daemon's `harness_catalogue_changed`, relayed by the
 * App). Every surface that shows a catalogue reads it again.
 */
export const HARNESS_CATALOGUE_CHANGED = "pdo:harness-catalogue-changed";

export function announceHarnessCatalogueChanged() {
  window.dispatchEvent(new Event(HARNESS_CATALOGUE_CHANGED));
}

/** Run `onChange` at each {@link HARNESS_CATALOGUE_CHANGED}. */
export function useHarnessCatalogueChanged(onChange: () => void, enabled = true) {
  useEffect(() => {
    if (!enabled) return;
    window.addEventListener(HARNESS_CATALOGUE_CHANGED, onChange);
    return () => window.removeEventListener(HARNESS_CATALOGUE_CHANGED, onChange);
  }, [onChange, enabled]);
}

/**
 * The harness catalog for surfaces that don't already hold `GET /settings` — the
 * node inspector, the Projet modal, the agentic profile modal (#586). Fetched on
 * mount and again whenever a catalogue was probed again: the embedded floor
 * shows until the first answer, and a failed fetch keeps the last one (the
 * daemon re-resolves authoritatively at spawn, so a stale picker never
 * mis-launches a node).
 *
 * Surfaces that already fetch settings (New Run, Settings) derive the catalog
 * with {@link harnessCatalog} directly rather than fetching twice.
 */
export function useHarnessCatalog(): HarnessCatalog {
  const [catalog, setCatalog] = useState<HarnessCatalog>(() => harnessCatalog(null));
  const mounted = useRef(true);
  const load = useCallback(() => {
    fetchHarnessCatalog()
      .then((served) => {
        if (mounted.current) setCatalog(served);
      })
      .catch(() => {
        /* keep the last catalogue — the picker is never empty */
      });
  }, []);
  useEffect(() => {
    mounted.current = true;
    load();
    return () => {
      mounted.current = false;
    };
  }, [load]);
  useHarnessCatalogueChanged(load);
  return catalog;
}

/**
 * « Refresh models »: probe a harness's catalogue again. While a probe of a
 * harness is in flight its button spins and a second click asks nothing (the
 * daemon would join the running probe anyway). Once it answers, every surface
 * of this tab reads the catalogue again; the other browsers hear it from the
 * daemon.
 */
export function useRefreshModels() {
  const [probing, setProbing] = useState<ReadonlySet<string>>(() => new Set());
  const inFlight = useRef(new Set<string>());
  const [failed, setFailed] = useState<Record<string, string>>({});
  const refresh = useCallback(async (harness: string) => {
    if (inFlight.current.has(harness)) return;
    inFlight.current.add(harness);
    setProbing(new Set(inFlight.current));
    setFailed((current) => {
      const rest = { ...current };
      delete rest[harness];
      return rest;
    });
    try {
      await refreshHarnessModels(harness);
      announceHarnessCatalogueChanged();
    } catch (cause) {
      const message = cause instanceof Error ? cause.message : "Refresh failed";
      setFailed((current) => ({ ...current, [harness]: message }));
    } finally {
      inFlight.current.delete(harness);
      setProbing(new Set(inFlight.current));
    }
  }, []);
  return { probing, failed, refresh };
}

/** The Operation behind « Refresh models » (`POST /settings/harnesses/{name}/refresh`). */
const REFRESH_CATALOGUE = { resource: "instance_settings", verb: { action: "refresh_catalogue" } } as const;

/** Why the caller may not refresh a harness's models (CORE-205: the button greyed), else `undefined`. */
export function useRefreshModelsRefused(): string | undefined {
  return useRefusal(REFRESH_CATALOGUE);
}
