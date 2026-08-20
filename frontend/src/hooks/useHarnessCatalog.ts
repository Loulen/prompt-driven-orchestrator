import { useEffect, useState } from "react";
import { fetchSettings } from "../api";
import { harnessCatalog } from "../lib/harness";
import type { HarnessCatalog } from "../lib/harness";

/**
 * The harness catalog for surfaces that don't already hold `GET /settings` — the
 * node inspector and the Projet modal (#586). One fetch per mount: the embedded
 * floor shows until it resolves, and a failed fetch keeps that fallback (the
 * daemon re-resolves authoritatively at spawn, so a stale picker never
 * mis-launches a node).
 *
 * Surfaces that already fetch settings (New Run, Settings) derive the catalog
 * with {@link harnessCatalog} directly rather than fetching twice.
 */
export function useHarnessCatalog(): HarnessCatalog {
  const [catalog, setCatalog] = useState<HarnessCatalog>(() => harnessCatalog(null));
  useEffect(() => {
    let cancelled = false;
    fetchSettings()
      .then((s) => {
        if (!cancelled) setCatalog(harnessCatalog(s.harness_descriptors));
      })
      .catch(() => {
        /* keep the floor fallback — the picker is never empty */
      });
    return () => {
      cancelled = true;
    };
  }, []);
  return catalog;
}
