import { useCallback, useEffect, useState } from "react";
import { fetchAgentProfiles } from "../api/agentProfiles";
import type { AgentProfile } from "../../shared/generated/AgentProfile";
import { rememberProfileNames } from "../model/profileNames";
import { refusalSentence } from "../../shared/authorization/refusal";

export const AGENT_PROFILES_CHANGED = "pdo:agent-profiles-changed";

export function announceAgentProfilesChanged() {
  window.dispatchEvent(new Event(AGENT_PROFILES_CHANGED));
}

export function useAgentProfiles(enabled = true) {
  const [profiles, setProfiles] = useState<AgentProfile[]>([]);
  const [error, setError] = useState<string | null>(null);
  // CORE-248: the sentence when the caller may not read the profiles.
  const [refused, setRefused] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    if (!enabled) return;
    try {
      const result = await fetchAgentProfiles();
      rememberProfileNames(result.profiles);
      setProfiles(result.profiles);
      setError(null);
      setRefused(null);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Failed to load agent profiles");
      const sentence = refusalSentence(cause);
      if (sentence !== null) {
        setProfiles([]);
        setRefused(sentence);
      }
    }
  }, [enabled]);

  useEffect(() => {
    if (!enabled) return;
    let cancelled = false;
    void fetchAgentProfiles()
      .then((result) => {
        if (!cancelled) {
          if (!Array.isArray(result.profiles)) throw new Error("Invalid agent profiles response");
          rememberProfileNames(result.profiles);
          setProfiles(result.profiles);
          setError(null);
          setRefused(null);
        }
      })
      .catch((cause) => {
        if (!cancelled) {
          setError(cause instanceof Error ? cause.message : "Failed to load agent profiles");
          setRefused(refusalSentence(cause));
        }
      });
    window.addEventListener(AGENT_PROFILES_CHANGED, refresh);
    return () => {
      cancelled = true;
      window.removeEventListener(AGENT_PROFILES_CHANGED, refresh);
    };
  }, [enabled, refresh]);

  return { profiles, error, refused, refresh };
}
