import { useCallback, useEffect } from "react";
import type { RunState } from "../../../shared/generated/RunState";
import type { RunTranscriptHit } from "../../../shared/generated/RunTranscriptHit";
import { useEditStore } from "../../../pipeline/store/editStore";
import { isSearchShortcut, RUN_SEARCH_EVENT, type RunSearchRequest } from "../../model/runSearch";
import { useRunSearchStore } from "../../store/runSearchStore";
import { useRunSearch } from "../../store/useRunSearch";
import RunSearchPanel from "./RunSearchPanel";

/**
 * The search over the transcripts of the Run on the canvas: opened by the
 * toolbar's loupe, by ⌘⇧F, or by « Search run » in a Transcript tab. A chosen
 * hit selects its node; the node's panel opens its Transcript at the hit.
 */
export default function RunSearchHost({
  run,
  onCloseInfo,
}: {
  run: RunState | null;
  onCloseInfo?: () => void;
}) {
  const runId = run?.run_id ?? null;
  const openOn = useRunSearchStore((s) => s.runId);
  const toggle = useRunSearchStore((s) => s.toggle);
  const open = useRunSearchStore((s) => s.open);
  const close = useRunSearchStore((s) => s.close);
  const choose = useRunSearchStore((s) => s.choose);
  const setSelection = useEditStore((s) => s.setSelection);
  useRunSearch();

  // Another Run on the canvas: the search of the previous one closes.
  useEffect(() => {
    if (openOn !== null && openOn !== runId) close();
  }, [openOn, runId, close]);

  useEffect(() => {
    if (!runId) return;
    const onKey = (e: KeyboardEvent) => {
      if (!isSearchShortcut(e)) return;
      e.preventDefault();
      toggle(runId);
    };
    const onAsk = (e: Event) => {
      const asked = (e as CustomEvent<RunSearchRequest>).detail?.runId;
      if (asked === runId) open(runId);
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener(RUN_SEARCH_EVENT, onAsk);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener(RUN_SEARCH_EVENT, onAsk);
    };
  }, [runId, toggle, open]);

  const nodeName = useCallback(
    (nodeId: string) => run?.node_defs?.find((d) => d.id === nodeId)?.name || nodeId,
    [run],
  );

  const onChoose = useCallback(
    (hit: RunTranscriptHit) => {
      if (!runId) return;
      choose(runId, hit);
      setSelection({ kind: "node", id: hit.node_id });
      onCloseInfo?.();
    },
    [runId, choose, setSelection, onCloseInfo],
  );

  if (!runId || openOn !== runId) return null;
  return <RunSearchPanel nodeName={nodeName} onChoose={onChoose} />;
}
