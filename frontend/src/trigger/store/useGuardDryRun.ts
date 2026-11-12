import { useState } from "react";
import { testGuard, type TestGuardResponse } from "../api/guard";

/**
 * Guard dry-run (#351): the last verdict, an in-flight flag, and an error. No
 * reset code needed — the detail panel is remounted per Trigger (`key` at the
 * App.tsx render site), so switching Triggers discards this state for free. The
 * fire-history refetch must NOT clear it.
 */
export function useGuardDryRun(
  guardCommand: string | undefined,
  targetRepo: string | undefined,
  projectId?: string,
) {
  const [result, setResult] = useState<TestGuardResponse | null>(null);
  const [testing, setTesting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Run the Trigger's *saved* guard with zero side effects and record the verdict
  // — the "why would this fire right now?" button. Never creates a Run, writes
  // fire history, or bumps next_fire_at.
  const run = async () => {
    const cmd = guardCommand?.trim();
    if (!cmd) return; // the button only renders when a guard is set
    setTesting(true);
    setError(null);
    try {
      setResult(await testGuard(cmd, targetRepo, projectId));
    } catch (e) {
      setResult(null);
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setTesting(false);
    }
  };

  return { result, testing, error, run };
}
