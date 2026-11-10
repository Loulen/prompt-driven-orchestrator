import { useCallback, useMemo } from "react";
import type { ReviewComment } from "../../shared/generated/ReviewComment";
import type { RunRefs } from "../../shared/generated/RunRefs";
import type { RunState } from "../../shared/generated/RunState";
import type { RefPair } from "../../run/model/runRefs";
import { stateCounts, stateCountsByPath } from "../model/commentStatus";
import { draftsForPair } from "../model/drafts";
import type { ReviewDraft } from "../model/drafts";
import { countsByPath, homeIds, mergeEntries, outdatedCountByPath } from "../model/entries";
import type { MappingMap, ReviewEntry } from "../model/entries";
import { unreadReplies } from "../model/seen";
import type { SeenMap } from "../model/seen";

interface Args {
  run: RunState | null;
  refs: RunRefs | null;
  pair: RefPair;
  drafts: ReviewDraft[];
  pathOrder: string[];
  mappings: MappingMap | undefined;
  seen: SeenMap;
}

/**
 * The comments of the displayed pair, merged and counted (#750, #751, #752):
 * drafts and sent comments in diff order, per file, per state; the unread ones
 * the bell jumps to; what `c` / `C` cycle through; the comments not at home on
 * this pair (written elsewhere, file absent here), listed greyed, never lost.
 */
export function useReviewEntries({ run, refs, pair, drafts, pathOrder, mappings, seen }: Args) {
  const entries = useMemo<ReviewEntry[]>(
    () => mergeEntries(drafts, run?.review_comments, pair, pathOrder, mappings),
    [drafts, run?.review_comments, pair, pathOrder, mappings],
  );
  const outdatedCounts = useMemo(() => outdatedCountByPath(entries), [entries]);
  const entriesByPath = useMemo(() => {
    const m = new Map<string, ReviewEntry[]>();
    for (const e of entries) {
      const list = m.get(e.anchor.path) ?? [];
      list.push(e);
      m.set(e.anchor.path, list);
    }
    return m;
  }, [entries]);
  const counts = useMemo(() => countsByPath(entries), [entries]);
  const states = useMemo(() => stateCountsByPath(entries), [entries]);
  const pairDrafts = useMemo(() => draftsForPair(drafts, pair), [drafts, pair]);
  const sentCount = entries.length - pairDrafts.length;
  const sentEntries = useMemo(() => entries.flatMap((e) => (e.kind === "sent" ? [e.comment] : [])), [entries]);
  const pairStates = useMemo(() => stateCounts(sentEntries), [sentEntries]);
  /**
   * Open comments of this pair with replies this browser has not seen, in diff
   * order — what the bell jumps to. A comment the agent resolved directly is
   * left out (nothing waits on the human), like the Diff tab badge.
   */
  const unreadComments = useMemo(
    () => sentEntries.filter((c) => c.status === "sent" && unreadReplies(c, seen) > 0),
    [sentEntries, seen],
  );
  /** What `c` / `C` cycle through: drafts and open / proposed comments — resolved are skipped. */
  const cycleEntries = useMemo(
    () => entries.filter((e) => e.kind === "draft" || e.comment.status !== "resolved"),
    [entries],
  );
  const home = useMemo(() => homeIds(entries), [entries]);
  const otherEntries = useMemo(() => {
    const label = (from: string, to: string) => {
      const name = (id: string) => refs?.refs.find((r) => r.id === id)?.label ?? id;
      return `${name(from)} → ${name(to)}`;
    };
    const out: { entry: ReviewEntry; pair: string; refPair: RefPair }[] = [];
    for (const d of drafts) {
      if (d.from === pair.from && d.to === pair.to) continue;
      out.push({ entry: { kind: "draft", anchor: d, draft: d }, pair: label(d.from, d.to), refPair: { from: d.from, to: d.to } });
    }
    for (const c of run?.review_comments ?? []) {
      if (home.has(c.id)) continue;
      out.push({
        entry: { kind: "sent", anchor: { path: c.path, side: c.side, line: c.line }, comment: c },
        pair: label(c.from_ref, c.to_ref),
        refPair: { from: c.from_ref, to: c.to_ref },
      });
    }
    return out;
  }, [drafts, run?.review_comments, pair, refs, home]);

  const pairLabel = useMemo(() => {
    const name = (id: string) => refs?.refs.find((r) => r.id === id)?.label ?? id;
    return `${name(pair.from)} → ${name(pair.to)}`;
  }, [refs, pair]);
  /** #752: the label of the ref a comment's anchored side was written against. */
  const writtenOnLabel = useCallback(
    (c: ReviewComment) => {
      const id = c.side === "old" ? c.from_ref : c.to_ref;
      const label = refs?.refs.find((r) => r.id === id)?.label ?? id;
      const sha = c.side === "old" ? c.from_sha : c.to_sha;
      return sha ? `${label} · ${sha.slice(0, 7)}` : label;
    },
    [refs],
  );

  return {
    entries,
    outdatedCounts,
    anyOutdated: outdatedCounts.size > 0,
    entriesByPath,
    counts,
    states,
    pairDrafts,
    sentCount,
    pairStates,
    unreadComments,
    cycleEntries,
    otherEntries,
    pairLabel,
    writtenOnLabel,
  };
}

export type ReviewEntries = ReturnType<typeof useReviewEntries>;
