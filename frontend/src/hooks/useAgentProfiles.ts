import { useCallback, useEffect, useState } from "react";
import { fetchAgentProfiles } from "../api";
import type { AgentProfile } from "../types";

export const AGENT_PROFILES_CHANGED = "pdo:agent-profiles-changed";

export function announceAgentProfilesChanged() {
  window.dispatchEvent(new Event(AGENT_PROFILES_CHANGED));
}

export function useAgentProfiles(enabled = true) {
  const [profiles, setProfiles] = useState<AgentProfile[]>([]);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    if (!enabled) return;
    try {
      const result = await fetchAgentProfiles();
      setProfiles(result.profiles);
      setError(null);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Failed to load agent profiles");
    }
  }, [enabled]);

  useEffect(() => {
    if (!enabled) return;
    let cancelled = false;
    void fetchAgentProfiles()
      .then((result) => {
        if (!cancelled) {
          if (!Array.isArray(result.profiles)) throw new Error("Invalid agent profiles response");
          setProfiles(result.profiles);
          setError(null);
        }
      })
      .catch((cause) => {
        if (!cancelled) {
          setError(cause instanceof Error ? cause.message : "Failed to load agent profiles");
        }
      });
    window.addEventListener(AGENT_PROFILES_CHANGED, refresh);
    return () => {
      cancelled = true;
      window.removeEventListener(AGENT_PROFILES_CHANGED, refresh);
    };
  }, [enabled, refresh]);

  return { profiles, error, refresh };
}
