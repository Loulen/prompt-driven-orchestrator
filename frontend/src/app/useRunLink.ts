import { useEffect, useRef } from "react";
import { fetchRun } from "../run/api/runs";
import { dismissTransientOverlays } from "../lib/overlays";
import { formatDeepLink, parseRunLink, parseRunTarget, usePages } from "../shared/extensions/deepLink";
import { ApiError } from "../shared/http/client";
import { toast } from "../shared/ui/toast";

/**
 * Follow the deep links toward a Run (`#/runs/<run_id>`, CORE-1), whoever wrote
 * them: the address bar on load, a markdown link, an extension's
 * `CommandHost.open` or its page setting `location.hash`. A known Run closes the
 * open extension page, gives way to any transient overlay and is selected as a
 * click in the list would; an unknown one says so in an error toast and changes
 * nothing. Either way the link leaves the address bar: the app keeps no Run in
 * its URL, and a stale one would outlive the next selection. A link toward a
 * node (`#/runs/<id>/nodes/<node>`) selects it once its Run is shown.
 */
export function useRunLink(
  onSelectRun: (runId: string) => void | Promise<void>,
  onSelectNode?: (nodeId: string) => void,
): void {
  const selectRef = useRef(onSelectRun);
  const nodeRef = useRef(onSelectNode);
  useEffect(() => {
    selectRef.current = onSelectRun;
    nodeRef.current = onSelectNode;
  }, [onSelectRun, onSelectNode]);

  useEffect(() => {
    // Only the last link followed counts: a late answer for an earlier one is dropped.
    let latest = 0;
    const follow = async () => {
      const target = parseRunTarget(window.location.hash);
      if (!target) return;
      const { runId, nodeId } = target;
      const attempt = ++latest;
      let failure: string | null = null;
      try {
        await fetchRun(runId);
      } catch (cause) {
        failure =
          cause instanceof ApiError && cause.status === 404
            ? `Run “${runId}” not found: this link can't be opened.`
            : `Run “${runId}” can't be opened: ${cause instanceof Error ? cause.message : String(cause)}`;
      }
      if (attempt !== latest || parseRunLink(window.location.hash) !== runId) return;
      const open = usePages.getState().open;
      const { pathname, search } = window.location;
      if (failure === null) {
        usePages.setState({ open: null, requested: null });
        dismissTransientOverlays();
        const selected = selectRef.current(runId);
        if (nodeId) void Promise.resolve(selected).then(() => nodeRef.current?.(nodeId));
      } else {
        toast(failure, "error");
      }
      const back = failure !== null && open ? formatDeepLink(open.extension, open.location, open.path) : `${pathname}${search}`;
      window.history.replaceState(window.history.state, "", back);
    };
    void follow();
    window.addEventListener("hashchange", follow);
    window.addEventListener("popstate", follow);
    return () => {
      latest = -1;
      window.removeEventListener("hashchange", follow);
      window.removeEventListener("popstate", follow);
    };
  }, []);
}
