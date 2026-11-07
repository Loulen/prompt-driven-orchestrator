import { useEffect, useState } from "react";
import { fetchNodeIO } from "../../run/api/nodes";
import { missingOutputs } from "../../run/model/approve";
import { isMarkCompletable } from "../../run/model/nodeStatus";
import type { PaletteSelection } from "./paletteHost";

/** While an output is missing, how often the open command field reads them again (the panel's cadence). */
export const MISSING_REFRESH_MS = 2000;

/**
 * The outputs the selected node has not written yet, read when the command
 * field opens on it, and again every 2 s while one is missing, so its Approve
 * greys and turns clickable as the Inspector's does: `null` while they are
 * first read, `[]` for anything but a node Approve applies to. A failed read
 * says nothing is missing: the daemon's guard still refuses an Approve that
 * comes too early, and the toast says why.
 */
export function useSelectionMissingOutputs(selection: PaletteSelection | null): readonly string[] | null {
  const target =
    selection?.kind === "node" && isMarkCompletable(selection.node.status, selection.run.status)
      ? { runId: selection.run.id, nodeId: selection.node.id, iter: selection.node.iter, status: selection.node.status }
      : null;
  const key = target ? `${target.runId}/${target.nodeId}/${target.iter}` : null;
  const [read, setRead] = useState<{ key: string; missing: string[] } | null>(null);
  const runId = target?.runId;
  const nodeId = target?.nodeId;
  const iter = target?.iter;
  const status = target?.status;
  useEffect(() => {
    if (key === null || runId === undefined || nodeId === undefined || iter === undefined) return;
    let live = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const readOnce = () =>
      fetchNodeIO(runId, nodeId, iter).then(
        (io) => {
          if (!live) return;
          const missing = missingOutputs(io.outputs);
          setRead({ key, missing });
          if (missing.length > 0) timer = setTimeout(readOnce, MISSING_REFRESH_MS);
        },
        () => live && setRead({ key, missing: [] }),
      );
    void readOnce();
    return () => {
      live = false;
      clearTimeout(timer);
    };
    // `status`: a node that changes status on the same iteration is read again.
  }, [key, runId, nodeId, iter, status]);
  if (key === null) return [];
  return read?.key === key ? read.missing : null;
}
