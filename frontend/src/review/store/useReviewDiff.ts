import { useEffect, useMemo, useRef, useState } from "react";
import type { MutableRefObject } from "react";
import type { RunState } from "../../shared/generated/RunState";
import type { StructuredDiff } from "../../shared/generated/StructuredDiff";
import { deliverySignature } from "../../run/model/runRefs";
import type { RefPair } from "../../run/model/runRefs";
import { fetchReviewComments } from "../api/comments";
import { fetchRunStructuredDiff } from "../api/diff";
import { mappingsOf, remapSummary, sentEntriesForPair } from "../model/entries";
import type { MappingMap } from "../model/entries";

/**
 * The loaded diff, tagged with the key it was fetched for (`from|to|reloadTick`):
 * a key mismatch *is* the loading state, so no effect ever writes "loading".
 */
export type Load =
  | { kind: "none" }
  | { kind: "ready"; key: string; diff: StructuredDiff }
  | { kind: "error"; key: string; message: string; status?: number };

interface Args {
  runId: string;
  run: RunState | null;
  runRef: MutableRefObject<RunState | null>;
  sigAtLoad: MutableRefObject<string | null>;
  pair: RefPair;
  defaults: RefPair;
  reloadTick: number;
  refsSettled: boolean;
}

/**
 * The structured diff of the displayed pair (#748, #749), and the daemon's
 * re-map of every sent comment onto it (#752): once the diff is in (the
 * mapping reads the same SHAs), and again whenever the set of comments changes
 * (a send, a replay). Keyed like the diff so a pair change never shows a stale
 * mapping: a key mismatch reads as "not mapped yet". After a re-map, a note
 * says `1 comment moved · 1 outdated` for a few seconds.
 */
export function useReviewDiff({ runId, run, runRef, sigAtLoad, pair, defaults, reloadTick, refsSettled }: Args) {
  const [load, setLoad] = useState<Load>({ kind: "none" });
  const [tipMoved, setTipMoved] = useState(false);
  const [mappings, setMappings] = useState<{ key: string; map: MappingMap } | null>(null);
  const [remapNote, setRemapNote] = useState<string | null>(null);
  const remapTimer = useRef<number | undefined>(undefined);
  /** The displayed files' paths, in diff order — the page keeps it current. */
  const pathOrderRef = useRef<string[]>([]);

  const isArchived = run?.status === "archived";
  const sameRef = pair.from === pair.to;
  const { from: pairFrom, to: pairTo } = pair;
  const loadKey = `${pairFrom}|${pairTo}|${reloadTick}`;
  const { from: defaultFrom, to: defaultTo } = defaults;
  useEffect(() => {
    if (isArchived || sameRef || !refsSettled) return;
    let stale = false;
    const key = `${pairFrom}|${pairTo}|${reloadTick}`;
    const query = pairFrom === defaultFrom && pairTo === defaultTo ? undefined : { from: pairFrom, to: pairTo };
    fetchRunStructuredDiff(runId, query)
      .then((d) => {
        if (stale) return;
        sigAtLoad.current = runRef.current ? deliverySignature(runRef.current.nodes) : null;
        setTipMoved(false);
        setLoad({ kind: "ready", key, diff: d });
      })
      .catch((e: unknown) => {
        if (stale) return;
        const status = (e as { status?: number })?.status;
        setLoad({ kind: "error", key, message: e instanceof Error ? e.message : String(e), status });
      });
    return () => {
      stale = true;
    };
  }, [runId, pairFrom, pairTo, reloadTick, isArchived, sameRef, refsSettled, defaultFrom, defaultTo, runRef, sigAtLoad]);
  /** What the current key has: the loaded diff, an error, or nothing yet. */
  const current = useMemo<Load>(
    () => (load.kind !== "none" && load.key === loadKey ? load : { kind: "none" }),
    [load, loadKey],
  );

  const commentIdsSig = (run?.review_comments ?? []).map((c) => c.id).join(",");
  const mappingKey = `${loadKey}|${commentIdsSig}`;
  const diffReady = current.kind === "ready";
  useEffect(() => {
    if (!diffReady || !commentIdsSig) return;
    let stale = false;
    const pair = { from: pairFrom, to: pairTo };
    fetchReviewComments(runId, pair)
      .then((res) => {
        if (stale) return;
        const map = mappingsOf(res.comments);
        setMappings({ key: mappingKey, map });
        // The header note, once per mapping: `1 comment moved · 1 outdated`, a few seconds.
        const summary = remapSummary(sentEntriesForPair(res.comments, pair, map, pathOrderRef.current));
        if (summary) {
          setRemapNote(summary);
          window.clearTimeout(remapTimer.current);
          remapTimer.current = window.setTimeout(() => setRemapNote(null), 5000);
        }
      })
      .catch(() => {
        // Without a mapping the comments show where they were written.
      });
    return () => {
      stale = true;
    };
  }, [runId, pairFrom, pairTo, diffReady, commentIdsSig, mappingKey]);
  const activeMappings = mappings && mappings.key === mappingKey ? mappings.map : undefined;
  useEffect(() => () => window.clearTimeout(remapTimer.current), []);

  return { shown: current, isArchived, sameRef, tipMoved, setTipMoved, activeMappings, remapNote, pathOrderRef };
}
