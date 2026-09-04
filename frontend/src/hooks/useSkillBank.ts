import { useCallback, useEffect, useState } from "react";
import { fetchSkillBank } from "../api";
import type { SkillBank } from "../types";

/**
 * Window bus fired after every write to the Banque de skills (#668), on the
 * model of `pdo:agent-profiles-changed`: the future tier selectors (#669+) read
 * the same list and must refresh after a paste / move / rename / delete here.
 */
export const SKILLS_CHANGED = "pdo:skills-changed";

export function announceSkillsChanged() {
  window.dispatchEvent(new Event(SKILLS_CHANGED));
}

const EMPTY: SkillBank = { skills: [], folders: [], root_path: "" };

export function useSkillBank(enabled = true) {
  const [bank, setBank] = useState<SkillBank>(EMPTY);
  const [loaded, setLoaded] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    if (!enabled) return;
    try {
      const result = await fetchSkillBank();
      if (!Array.isArray(result.skills) || !Array.isArray(result.folders)) {
        throw new Error("Invalid skill bank response");
      }
      setBank(result);
      setError(null);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Failed to load skills");
    } finally {
      setLoaded(true);
    }
  }, [enabled]);

  useEffect(() => {
    if (!enabled) return;
    let cancelled = false;
    // Initial load inline (a subscription-style callback, not a synchronous
    // setState in the effect body); the bus re-uses `refresh`.
    fetchSkillBank()
      .then((result) => {
        if (cancelled) return;
        if (!Array.isArray(result.skills) || !Array.isArray(result.folders)) {
          throw new Error("Invalid skill bank response");
        }
        setBank(result);
        setError(null);
        setLoaded(true);
      })
      .catch((cause) => {
        if (cancelled) return;
        setError(cause instanceof Error ? cause.message : "Failed to load skills");
        setLoaded(true);
      });
    window.addEventListener(SKILLS_CHANGED, refresh);
    return () => {
      cancelled = true;
      window.removeEventListener(SKILLS_CHANGED, refresh);
    };
  }, [enabled, refresh]);

  return { bank, loaded, error, refresh };
}
