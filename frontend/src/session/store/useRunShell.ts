import { useCallback, useState } from "react";
import { openRunShell } from "../api/sessions";

/** The run shell opened from a finished Run (#316 / ADR-0021, #598). */
export interface OpenRunShell {
  runId: string;
  /** The tmux session to attach the shell modal to. */
  session: string;
}

/**
 * The ad-hoc bash shell of a terminal Run: open (or re-attach) it, and close its
 * modal. Opening is silent on failure — the daemon's gate may 409 if the
 * worktree vanished out-of-band, and nothing is actionable from a button.
 */
export function useRunShell() {
  const [shell, setShell] = useState<OpenRunShell | null>(null);

  const open = useCallback(async (runId: string) => {
    try {
      const { session } = await openRunShell(runId);
      setShell({ runId, session });
    } catch {
      // See above: nothing actionable.
    }
  }, []);

  const close = useCallback(() => setShell(null), []);

  return { shell, open, close };
}
