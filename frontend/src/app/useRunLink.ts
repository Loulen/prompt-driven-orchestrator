import { useCallback, useEffect, useRef } from "react";
import { useEditStore } from "../pipeline/store/editStore";
import { useNodeLinkFocus } from "../run/store/nodeLinkFocus";
import { fetchRun } from "../run/api/runs";
import type { RunState } from "../shared/generated/RunState";
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
 * node (`#/runs/<id>/nodes/<node>[/iter/<n>]`) selects it, at that iteration,
 * once its Run is shown; a node the Run does not hold leaves the Run alone.
 */
export function useRunLink(
  onSelectRun: (runId: string) => void | Promise<void>,
  onSelectNode?: (nodeId: string, iter: number | null, runId: string) => void,
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
      const { runId, nodeId, iter } = target;
      const attempt = ++latest;
      let failure: string | null = null;
      let run: RunState | null = null;
      try {
        run = await fetchRun(runId);
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
        if (nodeId && run && runHoldsNode(run, nodeId)) {
          void Promise.resolve(selected).then(() => nodeRef.current?.(nodeId, iter, runId));
        }
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

/** Whether `run` holds the node `nodeId`: started, or defined by its pipeline. */
function runHoldsNode(run: RunState, nodeId: string): boolean {
  return nodeId in (run.nodes ?? {}) || (run.node_defs ?? []).some((def) => def.id === nodeId);
}

/**
 * What a link toward a node does once its Run is shown (CORE-203): select the
 * node on the canvas, so its detail and its terminal show, Pipeline info
 * giving way (`closeInfo`), and ask its panel for the iteration the link names.
 */
export function useLinkedNodeSelection(
  closeInfo: (open: false) => void,
): (nodeId: string, iter: number | null, runId: string) => void {
  const setSelection = useEditStore((s) => s.setSelection);
  const openNodeLink = useNodeLinkFocus((s) => s.open);
  return useCallback(
    (nodeId, iter, runId) => {
      closeInfo(false);
      setSelection({ kind: "node", id: nodeId });
      openNodeLink(runId, nodeId, iter);
    },
    [closeInfo, setSelection, openNodeLink],
  );
}
