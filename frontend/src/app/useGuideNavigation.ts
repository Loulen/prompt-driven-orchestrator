import { useCallback, useMemo } from "react";
import { useEditStore } from "../pipeline/store/editStore";
import type { GuideNavigation } from "../platform/components/GuidePanel";
import type { GuideTarget } from "../platform/model/mountedPage";
import { followDeepLink } from "../shared/extensions/deepLink";
import type { RunListEntry } from "../shared/generated/RunListEntry";

/**
 * Where a reading guide's links into PDO lead, from the app shell: a Run opens in its
 * tab (the page's tab stays open beside it), a node or an artifact is selected on its
 * Run's canvas with its I/O, a Review file opens the Review in a browser tab, an
 * extension's page opens as its deep link does.
 */
export function useGuideNavigation(
  runs: RunListEntry[],
  onSelectRun: (runId: string) => Promise<void>,
): GuideNavigation {
  const setSelection = useEditStore((s) => s.setSelection);
  const runName = useCallback(
    (runId: string) => runs.find((run) => run.run_id === runId)?.name || null,
    [runs],
  );
  const openInPdo = useCallback(
    (target: GuideTarget) => {
      switch (target.kind) {
        case "run":
          void onSelectRun(target.runId);
          return;
        case "node":
        case "artifact":
          void onSelectRun(target.runId).then(() => setSelection({ kind: "node", id: target.nodeId }));
          return;
        case "review":
          window.open(target.href, "_blank", "noopener");
          return;
        case "extension":
          followDeepLink(target.href);
          return;
        default:
          return;
      }
    },
    [onSelectRun, setSelection],
  );
  return useMemo(() => ({ runName, openInPdo }), [runName, openInPdo]);
}
