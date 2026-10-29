import { useCallback, useState } from "react";
import type { RefPair } from "../../run/model/runRefs";
import { readViewed, toggleViewed, writeViewed } from "../model/viewed";

/**
 * The files of the pair on screen marked « Viewed » in this browser (the
 * forge's checkbox on a file card): read when the pair changes, written on
 * each toggle.
 */
export function useReviewViewed(runId: string, pair: RefPair) {
  const key = `${runId}|${pair.from}|${pair.to}`;
  const [state, setState] = useState(() => ({ key, viewed: readViewed(runId, pair) }));
  // A new pair reads its own marks (state adjusted during render, no effect).
  const current = state.key === key ? state : { key, viewed: readViewed(runId, pair) };
  if (current !== state) setState(current);

  const toggle = useCallback(
    (path: string) => {
      setState((prev) => {
        const base = prev.key === key ? prev.viewed : readViewed(runId, pair);
        const viewed = toggleViewed(base, path);
        writeViewed(runId, pair, viewed);
        return { key, viewed };
      });
    },
    // `pair` is read through `key`.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [key, runId],
  );

  return { viewed: current.viewed, toggle };
}
