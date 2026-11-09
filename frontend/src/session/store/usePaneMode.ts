import { useCallback, useEffect, useState } from "react";
import { fetchPane } from "../api/sessions";
import {
  REAPED_STATUSES,
  type FrozenPane,
  type PaneMode,
  type PaneSource,
} from "../model/terminalPane";

/**
 * Whether a terminal attaches live or shows the pane the daemon froze (#617).
 *
 * Decided **once per session identity**, not on every status change: a node that
 * settles under the user's eyes keeps the live buffer it already has (the daemon
 * may not even have frozen the snapshot yet), while a node opened after it
 * settled reads the snapshot. A retry spawns a new iteration — a new session name
 * — so the live path is re-entered there, which is what this re-decision is for.
 *
 * `reprobe` takes a live terminal back to the probe: for the node that settled
 * under the user's eyes once its socket fell. The daemon froze the pane before
 * it killed the session, so the snapshot is there to read — instead of a live
 * xterm showing tmux's `[exited]` and a « disconnected » badge.
 */
export function usePaneMode(
  session: string,
  status: string | undefined,
  paneSource: PaneSource | undefined,
): { mode: PaneMode; frozen: FrozenPane | null; reprobe: () => void } {
  const reaped =
    paneSource !== undefined &&
    status !== undefined &&
    REAPED_STATUSES.has(status);

  const [frozen, setFrozen] = useState<FrozenPane | null>(null);
  const [mode, setMode] = useState<PaneMode>(reaped ? "probing" : "live");
  const [decidedFor, setDecidedFor] = useState(session);
  if (decidedFor !== session) {
    setDecidedFor(session);
    setMode(reaped ? "probing" : "live");
    setFrozen(null);
  }

  // #617: read the frozen pane before opening any socket. `GET …/pane` answers
  // `live` when the session is somehow still up (then we attach as usual) and
  // `snapshot` for the reaped-and-frozen case this exists for. It never resurrects
  // a terminal iteration, so asking is free of side effects — which is why the
  // probe is gated on a reaped status rather than run for every node.
  // Deps are the three primitives, not the `paneSource` object: the detail panel
  // re-renders on every I/O poll tick, so an object identity here would cancel and
  // restart the probe once a second and never land.
  const paneRunId = paneSource?.runId;
  const paneNodeId = paneSource?.nodeId;
  const paneIter = paneSource?.iter;
  useEffect(() => {
    if (mode !== "probing") return;
    if (paneRunId === undefined || paneNodeId === undefined || paneIter === undefined) {
      return;
    }
    let cancelled = false;
    fetchPane(paneRunId, paneNodeId, paneIter)
      .then((pane) => {
        if (cancelled) return;
        if (pane.source === "live" || pane.source === "resumed") {
          setMode("live");
          return;
        }
        setFrozen({
          content: pane.content,
          preserved: pane.source === "snapshot",
        });
        setMode("frozen");
      })
      .catch(() => {
        if (cancelled) return;
        setFrozen({ content: "Pane unavailable.", preserved: false });
        setMode("frozen");
      });
    return () => {
      cancelled = true;
    };
  }, [mode, paneRunId, paneNodeId, paneIter]);

  const reprobe = useCallback(() => setMode((current) => (current === "live" ? "probing" : current)), []);

  return { mode, frozen, reprobe };
}
