import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  ArrowLeftRight,
  ArrowLeft,
  Archive,
  Bell,
  Columns2,
  Eye,
  EyeOff,
  History,
  ListMinus,
  MessageSquare,
  PanelLeftOpen,
  Rows3,
  SquareArrowOutUpRight,
} from "lucide-react";
import "@git-diff-view/react/styles/diff-view.css";
import {
  fetchReviewComments,
  fetchRun,
  fetchRunRefs,
  fetchRunStructuredDiff,
  reopenReviewComment,
  resolveReviewComment,
  sendReviewComments,
} from "../api";
import { useDaemonSocket } from "../hooks/useDaemonSocket";
import type { ReviewComment, RunRefs, RunState, StructuredDiff } from "../types";
import RefPicker from "../components/review/RefPicker";
import ReviewFileList from "../components/review/ReviewFileList";
import ReviewFileCard from "../components/review/ReviewFileCard";
import type { ReviewCommentsApi } from "../components/review/ReviewFileCard";
import ReviewSendBar from "../components/review/ReviewSendBar";
import { StateIcon } from "../components/review/CommentCard";
import {
  addDraft,
  allSeen,
  authorLabel,
  countsByPath,
  draftsForPair,
  homeIds,
  mappingsOf,
  markSeen as markSeenIn,
  mergeEntries,
  outdatedCountByPath,
  plural,
  readDrafts,
  readSeen,
  remapSummary,
  removeDrafts,
  sendDisabledReason as sendReasonOf,
  sentEntriesForPair,
  SHOW_OUTDATED_KEY,
  stateCounts,
  stateCountsByPath,
  toSendInputs,
  unreadReplies,
  updateDraft,
  writeDrafts,
  writeSeen,
} from "../lib/reviewComments";
import type { Anchor, MappingMap, ReviewDraft, ReviewEntry, SeenMap } from "../lib/reviewComments";
import {
  defaultPair,
  hasExplicitPair,
  defaultCollapsed,
  deliveryOfPair,
  deliverySignature,
  filePath,
  isDefaultPair,
  WORKTREE_ID,
  pairFromSearch,
  readListOpen,
  readView,
  reconcilePair,
  reviewUrl,
  writeListOpen,
  writeView,
} from "../lib/runRefs";
import type { RefPair, ViewMode } from "../lib/runRefs";

/**
 * The **Review page** (#749, ADR-0067; CONTEXT.md § "Relecture de diff"): a
 * dedicated URL — `/runs/<id>/review?from=<ref>&to=<ref>` — rendering the diff
 * between two **Run refs** the way a GitHub PR's "Files changed" does: files on
 * the left with their stats, side-by-side by default (unified toggle remembered
 * on this browser), context expansion between hunks, and a `source →
 * destination` pair picked among the Run's refs (fork point, Run tip, every
 * node delivery's before/after, a running node's live branch).
 *
 * Mounted full-window by `main.tsx` when the path matches; the app has no
 * router. The URL carries stable ref ids, never SHAs.
 *
 * Review comments (#750, ADR-0067 §2): the `+` on a line number opens an inline
 * editor; a saved **draft** lives in this browser (localStorage per Run) until
 * it is **sent** — per card, from the editor, or all at once from the sticky
 * footer bar / the toolbar pill. Sending posts the batch to the daemon, which
 * starts the manager on demand and hands it **one message**; the comments come
 * back as immutable Run events (`review_comments` in the projected state,
 * refreshed over the WebSocket). A Run whose branch is gone shows why sending
 * is disabled instead of a dead button.
 *
 * The conversation (#751, ADR-0067 §4): an agent's `pdo review reply` lands as
 * a `review_comment_replied` event, pushed live — the reply appears inline
 * under its comment, the card flashes, the reply carries a blue dot until the
 * card is seen; when it is off-screen a blue bell pill in the header jumps to
 * it. A `--resolved` reply is a **resolution proposed** (amber) the human
 * settles with Resolve / Reopen; a resolved comment collapses (green) and keeps
 * Reopen. The header counts open / proposed / resolved (icon + count, only when
 * non-zero) and an eye toggle hides resolved cards; `c` / `C` skip them.
 * Opening this page marks every reply seen for this browser (localStorage) —
 * that is what clears the Diff tab's unread badge; the per-card dots clear as
 * each card is looked at.
 *
 * Reported / outdated comments (#752, ADR-0067 §5): once the diff of a pair is
 * loaded, the page asks the daemon to **re-map** every sent comment onto that
 * pair (`GET …/review/comments?from&to`, one read, nothing written). A comment
 * whose line is unchanged sits at its new position (a `↳ from R212` chip only
 * when the number differs); one whose line changed is **outdated**: collapsed in
 * a group at the top of its file card, original hunk inside, still answerable
 * and resolvable. A note in the top bar says `1 comment moved · 1 outdated` for a
 * few seconds after a re-map; a `Show outdated` toggle hides those cards, never
 * the counts. Drafts are never re-mapped.
 */

const SHOW_RESOLVED_KEY = "pdo.review.showResolved";
/** How long a card keeps its outline flash after a live reply. */
const FLASH_MS = 2400;

interface Props {
  runId: string;
}

/**
 * The loaded diff, tagged with the key it was fetched for (`from|to|reloadTick`):
 * a key mismatch *is* the loading state, so no effect ever writes "loading".
 */
type Load =
  | { kind: "none" }
  | { kind: "ready"; key: string; diff: StructuredDiff }
  | { kind: "error"; key: string; message: string; status?: number };

const NARROW_QUERY = "(max-width: 900px)";

export default function ReviewPage({ runId }: Props) {
  const [run, setRun] = useState<RunState | null>(null);
  const [runError, setRunError] = useState<string | null>(null);
  const [refs, setRefs] = useState<RunRefs | null>(null);
  // #835: the default pair is the daemon's (fork → worktree while the Run's
  // worktree exists, fork → tip after). Until the refs are in, the URL pair
  // is read against the fallback; the diff waits for the refs so a URL with no
  // pair loads the real default once, not tip then worktree.
  const [refsSettled, setRefsSettled] = useState(false);
  const urlExplicit = useRef(hasExplicitPair(window.location.search));
  const [pair, setPairState] = useState<RefPair>(() => pairFromSearch(window.location.search));
  const [notice, setNotice] = useState<string | null>(null);
  const [view, setView] = useState<ViewMode>(() => readView());
  const [narrow, setNarrow] = useState<boolean>(() => window.matchMedia?.(NARROW_QUERY).matches ?? false);
  const [listOpen, setListOpen] = useState<boolean>(() => readListOpen());
  const [filter, setFilter] = useState("");
  const [collapsed, setCollapsed] = useState<Set<string> | null>(null);
  const [currentIdx, setCurrentIdx] = useState(0);
  const [load, setLoad] = useState<Load>({ kind: "none" });
  const [reloadTick, setReloadTick] = useState(0);
  const [tipMoved, setTipMoved] = useState(false);
  const [nodeDelivered, setNodeDelivered] = useState<{ nodeId: string; iter: number } | null>(null);
  // --- Review comments (#750) --------------------------------------------------
  const [drafts, setDrafts] = useState<ReviewDraft[]>(() => readDrafts(runId));
  const [editingKey, setEditingKey] = useState<string | null>(null);
  const [sendingKeys, setSendingKeys] = useState<ReadonlySet<string>>(() => new Set());
  const [startingManager, setStartingManager] = useState(false);
  const [toast, setToast] = useState<{ text: string; error?: boolean } | null>(null);
  const [commentCursor, setCommentCursor] = useState(-1);
  const toastTimer = useRef<number | undefined>(undefined);
  // --- The conversation (#751) ----------------------------------------------------
  /** What this browser had seen BEFORE this page opened — the unread dots read against it. */
  const [seen, setSeen] = useState<SeenMap>(() => readSeen(runId));
  const [showResolved, setShowResolved] = useState<boolean>(() => {
    try {
      return localStorage.getItem(SHOW_RESOLVED_KEY) !== "false";
    } catch {
      return true;
    }
  });
  const [deciding, setDeciding] = useState<ReadonlySet<string>>(() => new Set());
  // --- Reported / outdated (#752) ---------------------------------------------------
  /** The daemon's mapping of every sent comment onto the displayed pair, tagged with what it was fetched for. */
  const [mappings, setMappings] = useState<{ key: string; map: MappingMap } | null>(null);
  const [showOutdated, setShowOutdated] = useState<boolean>(() => {
    try {
      return localStorage.getItem(SHOW_OUTDATED_KEY) !== "false";
    } catch {
      return true;
    }
  });
  const [remapNote, setRemapNote] = useState<string | null>(null);
  const remapTimer = useRef<number | undefined>(undefined);
  const [flashing, setFlashing] = useState<ReadonlySet<string>>(() => new Set());
  const flashTimers = useRef<Map<string, number>>(new Map());
  const openedSeenWritten = useRef(false);

  const mainRef = useRef<HTMLDivElement | null>(null);
  const filterRef = useRef<HTMLInputElement | null>(null);
  const cardEls = useRef<Map<string, HTMLDivElement>>(new Map());
  const runRef = useRef<RunState | null>(null);
  const sigAtLoad = useRef<string | null>(null);
  const pairRef = useRef(pair);
  useEffect(() => {
    pairRef.current = pair;
  }, [pair]);

  const isArchived = run?.status === "archived";
  const effectiveView: ViewMode = narrow ? "unified" : view;

  // --- Run + refs -----------------------------------------------------------
  useEffect(() => {
    let stale = false;
    fetchRun(runId)
      .then((r) => {
        if (stale) return;
        runRef.current = r;
        setRun(r);
        // The signature at first sight, if the diff loaded before the Run did.
        if (sigAtLoad.current === null) sigAtLoad.current = deliverySignature(r.nodes);
      })
      .catch((e: unknown) => {
        if (!stale) setRunError(e instanceof Error ? e.message : String(e));
      });
    fetchRunRefs(runId)
      .then((r) => {
        if (stale) return;
        setRefs(r);
        // No pair in the URL: follow the daemon's default (#835).
        if (!urlExplicit.current) setPairState(defaultPair(r));
        setRefsSettled(true);
      })
      .catch(() => {
        // The pickers show ids; the diff loads with the URL pair anyway.
        if (!stale) setRefsSettled(true);
      });
    return () => {
      stale = true;
    };
  }, [runId, reloadTick]);

  useEffect(() => {
    if (run) document.title = `Review · ${run.pipeline_name} · ${runId}`;
  }, [run, runId]);

  // Opening the Review clears the Diff tab's unread badge (spec): every reply of
  // the Run counts as seen for this browser from now on. The `seen` snapshot the
  // dots read against stays what it was at open, so the page still points at
  // what is new.
  useEffect(() => {
    if (!run || openedSeenWritten.current) return;
    openedSeenWritten.current = true;
    writeSeen(runId, allSeen(run.review_comments, readSeen(runId)));
  }, [run, runId]);
  useEffect(() => {
    const timers = flashTimers.current;
    return () => timers.forEach((t) => window.clearTimeout(t));
  }, []);

  // Validate the URL pair once the refs are known; a bad ref falls back with a notice.
  const reconciledFor = useRef<RunRefs | null>(null);
  const defaults = useMemo(() => defaultPair(refs), [refs]);
  useEffect(() => {
    if (!refs || reconciledFor.current === refs) return;
    reconciledFor.current = refs;
    const { pair: next, notice: n } = reconcilePair(pairRef.current, refs);
    if (n) {
      setNotice(n);
      setPairState(next);
      window.history.replaceState(null, "", reviewUrl(runId, next, defaultPair(refs)));
    }
  }, [refs, runId]);

  // --- Diff -------------------------------------------------------------------
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
  }, [runId, pairFrom, pairTo, reloadTick, isArchived, sameRef, refsSettled, defaultFrom, defaultTo]);
  /** What the current key has: the loaded diff, an error, or nothing yet. */
  const current = useMemo<Load>(
    () => (load.kind !== "none" && load.key === loadKey ? load : { kind: "none" }),
    [load, loadKey],
  );

  // #752: re-map the sent comments onto the displayed pair — once its diff is in
  // (the mapping reads the same SHAs), and again whenever the set of comments
  // changes (a send, a replay). Keyed like the diff so a pair change never shows
  // a stale mapping: a key mismatch reads as "not mapped yet".
  const commentIdsSig = (run?.review_comments ?? []).map((c) => c.id).join(",");
  const mappingKey = `${loadKey}|${commentIdsSig}`;
  const diffReady = current.kind === "ready";
  const pathOrderRef = useRef<string[]>([]);
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

  // --- Live: the Run's WebSocket, only to detect "tip moved" / "node delivered".
  const { subscribe } = useDaemonSocket();
  useEffect(() => {
    return subscribe((msg) => {
      if (msg.type !== "event" || !msg.event || msg.event.run_id !== runId) return;
      const ev = msg.event;
      fetchRun(runId)
        .then((r) => {
          const before = runRef.current;
          runRef.current = r;
          setRun(r);
          const sig = deliverySignature(r.nodes);
          if (sigAtLoad.current !== null && sig !== sigAtLoad.current) setTipMoved(true);
          // #751: a reply landed live — flash its card for a moment. The dot
          // follows from `seen` (the snapshot does not know this reply).
          if (before) {
            const prevReplies = new Map((before.review_comments ?? []).map((c) => [c.id, c.replies?.length ?? 0]));
            const grown = (r.review_comments ?? []).filter((c) => (c.replies?.length ?? 0) > (prevReplies.get(c.id) ?? 0));
            if (grown.length > 0) {
              setFlashing((f) => new Set([...f, ...grown.map((c) => c.id)]));
              for (const c of grown) {
                window.clearTimeout(flashTimers.current.get(c.id));
                flashTimers.current.set(
                  c.id,
                  window.setTimeout(() => {
                    setFlashing((f) => {
                      const next = new Set(f);
                      next.delete(c.id);
                      return next;
                    });
                  }, FLASH_MS),
                );
              }
            }
          }
        })
        .catch(() => {});
      if (ev.kind === "node_delivered" && ev.node_id && pairRef.current.to === `live:${ev.node_id}`) {
        setNodeDelivered({ nodeId: ev.node_id, iter: ev.iter ?? 1 });
      }
    });
  }, [subscribe, runId]);

  // --- URL ↔ pair -------------------------------------------------------------
  const setPair = useCallback(
    (next: RefPair) => {
      urlExplicit.current = true;
      setPairState(next);
      setCollapsed(null);
      setCurrentIdx(0);
      setNotice(null);
      setNodeDelivered(null);
      window.history.replaceState(null, "", reviewUrl(runId, next, defaults));
      mainRef.current?.scrollTo?.({ top: 0 });
    },
    [runId, defaults],
  );
  useEffect(() => {
    const onPop = () => {
      urlExplicit.current = hasExplicitPair(window.location.search);
      setPairState(pairFromSearch(window.location.search, defaults));
    };
    window.addEventListener("popstate", onPop);
    return () => window.removeEventListener("popstate", onPop);
  }, [defaults]);

  // --- View persistence -------------------------------------------------------
  const changeView = (v: ViewMode) => {
    setView(v);
    writeView(v);
  };
  const toggleList = () => {
    setListOpen((o) => {
      writeListOpen(!o);
      return !o;
    });
  };
  useEffect(() => {
    const mq = window.matchMedia?.(NARROW_QUERY);
    if (!mq) return;
    const onChange = () => setNarrow(mq.matches);
    mq.addEventListener?.("change", onChange);
    return () => mq.removeEventListener?.("change", onChange);
  }, []);

  // --- Files ------------------------------------------------------------------
  const files = useMemo(() => (current.kind === "ready" ? current.diff.files : []), [current]);
  const collapsedSet = useMemo(() => collapsed ?? defaultCollapsed(files), [collapsed, files]);
  const toggleFile = (path: string) => {
    const next = new Set(collapsedSet);
    if (next.has(path)) next.delete(path);
    else next.add(path);
    setCollapsed(next);
  };
  const collapseAll = () => {
    const anyOpen = files.some((f) => !collapsedSet.has(filePath(f)));
    setCollapsed(anyOpen ? new Set(files.map(filePath)) : new Set());
  };

  const registerEl = useCallback((path: string, el: HTMLDivElement | null) => {
    if (el) cardEls.current.set(path, el);
    else cardEls.current.delete(path);
  }, []);

  const onScroll = useCallback(() => {
    const el = mainRef.current;
    if (!el) return;
    const top = el.scrollTop;
    let idx = 0;
    files.forEach((f, i) => {
      const card = cardEls.current.get(filePath(f));
      if (card && card.offsetTop <= top + 40) idx = i;
    });
    setCurrentIdx(idx);
  }, [files]);

  const goFile = useCallback(
    (index: number) => {
      const f = files[index];
      if (!f) return;
      const path = filePath(f);
      if (collapsedSet.has(path)) {
        const next = new Set(collapsedSet);
        next.delete(path);
        setCollapsed(next);
      }
      setCurrentIdx(index);
      const card = cardEls.current.get(path);
      const el = mainRef.current;
      if (card && el) el.scrollTo?.({ top: Math.max(0, card.offsetTop - 8), behavior: "smooth" });
    },
    [files, collapsedSet],
  );

  const goHunk = useCallback((dir: 1 | -1) => {
    const el = mainRef.current;
    if (!el) return;
    const rows = Array.from(el.querySelectorAll<HTMLElement>('tr[data-state="hunk"]'));
    const base = el.getBoundingClientRect().top + 48;
    const target =
      dir === 1
        ? rows.find((r) => r.getBoundingClientRect().top > base + 4)
        : [...rows].reverse().find((r) => r.getBoundingClientRect().top < base - 4);
    if (target) el.scrollBy?.({ top: target.getBoundingClientRect().top - base, behavior: "smooth" });
  }, []);

  // --- Review comments (#750) --------------------------------------------------
  const showToast = useCallback((text: string, error = false) => {
    setToast({ text, error });
    window.clearTimeout(toastTimer.current);
    toastTimer.current = window.setTimeout(() => setToast(null), error ? 6000 : 3600);
  }, []);
  useEffect(() => () => window.clearTimeout(toastTimer.current), []);

  const persistDrafts = useCallback(
    (next: ReviewDraft[]) => {
      setDrafts(next);
      writeDrafts(runId, next);
    },
    [runId],
  );

  const pathOrder = useMemo(() => files.map(filePath), [files]);
  useEffect(() => {
    pathOrderRef.current = pathOrder;
  }, [pathOrder]);
  const entries = useMemo<ReviewEntry[]>(
    () => mergeEntries(drafts, run?.review_comments, pair, pathOrder, activeMappings),
    [drafts, run?.review_comments, pair, pathOrder, activeMappings],
  );
  const outdatedCounts = useMemo(() => outdatedCountByPath(entries), [entries]);
  const anyOutdated = outdatedCounts.size > 0;
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
  /** Comments not at home on this pair (written elsewhere, file absent here): listed greyed, never lost. */
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

  const sendDisabledReason = sendReasonOf(run, refs);
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
  const toggleShowOutdated = () => {
    setShowOutdated((v) => {
      try {
        localStorage.setItem(SHOW_OUTDATED_KEY, String(!v));
      } catch {
        // Remembered for the session only.
      }
      return !v;
    });
  };

  /** Scroll the anchored line of an entry into view, expanding its file first. */
  const jumpTo = useCallback(
    (anchor: Anchor) => {
      if (collapsedSet.has(anchor.path)) {
        const next = new Set(collapsedSet);
        next.delete(anchor.path);
        setCollapsed(next);
      }
      const attempt = (left: number) => {
        const card = cardEls.current.get(anchor.path);
        const el =
          card?.querySelector<HTMLElement>(`td.diff-line-${anchor.side}-num > span[data-line-num="${anchor.line}"]`) ??
          card?.querySelector<HTMLElement>(`span[data-line-${anchor.side}-num="${anchor.line}"]`);
        if (el) el.closest("tr")?.scrollIntoView?.({ behavior: "smooth", block: "center" });
        else if (card && left > 0) window.setTimeout(() => attempt(left - 1), 60);
        else if (card) {
          const main = mainRef.current;
          if (main) main.scrollTo?.({ top: Math.max(0, card.offsetTop - 8), behavior: "smooth" });
        }
      };
      attempt(5);
    },
    [collapsedSet],
  );
  const jumpComment = useCallback(
    (dir: 1 | -1) => {
      if (cycleEntries.length === 0) return;
      const next = (commentCursor + dir + cycleEntries.length) % cycleEntries.length;
      setCommentCursor(next);
      jumpTo(cycleEntries[next].anchor);
    },
    [cycleEntries, commentCursor, jumpTo],
  );

  // --- The conversation (#751) ----------------------------------------------------
  const markSeen = useCallback(
    (c: ReviewComment) => {
      setSeen((prev) => markSeenIn(prev, c));
      // Storage too: another tab's Diff badge must not keep counting this one.
      writeSeen(runId, markSeenIn(readSeen(runId), c));
    },
    [runId],
  );
  /** The bell: jump to the first comment with an unread reply and mark it seen. */
  const jumpUnread = useCallback(() => {
    const target = unreadComments[0];
    if (!target) return;
    jumpTo({ path: target.path, side: target.side, line: target.line });
    markSeen(target);
  }, [unreadComments, jumpTo, markSeen]);
  const toggleShowResolved = () => {
    setShowResolved((v) => {
      try {
        localStorage.setItem(SHOW_RESOLVED_KEY, String(!v));
      } catch {
        // Remembered for the session only.
      }
      return !v;
    });
  };
  const decide = useCallback(
    async (c: ReviewComment, verb: "resolve" | "reopen") => {
      if (deciding.has(c.id)) return;
      setDeciding((d) => new Set([...d, c.id]));
      try {
        const res = await (verb === "resolve" ? resolveReviewComment(runId, c.id) : reopenReviewComment(runId, c.id));
        markSeen(res.comment);
        try {
          const r = await fetchRun(runId);
          runRef.current = r;
          setRun(r);
        } catch {
          // The WebSocket refresh lands anyway.
        }
        if (!res.changed) {
          showToast(`${c.id} was already ${verb === "resolve" ? "resolved" : "open"}.`);
        } else if (verb === "resolve") {
          showToast(`${c.id} resolved — collapsed; Reopen stays in its footer.`);
        } else if (c.status === "resolved") {
          showToast(`${c.id} reopened — back to open, the agent sees it in pdo review list.`);
        } else {
          showToast(`${c.id}: proposal declined — the comment stays open for ${authorLabel(res.comment.replies?.at(-1)?.author ?? "agent")}.`);
        }
      } catch (e: unknown) {
        showToast(`${verb === "resolve" ? "Resolve" : "Reopen"} failed — ${e instanceof Error ? e.message : String(e)}`, true);
      } finally {
        setDeciding((d) => {
          const next = new Set(d);
          next.delete(c.id);
          return next;
        });
      }
    },
    [deciding, runId, markSeen, showToast],
  );

  /** Send `keys` out of `source` (defaults to the current drafts; `sendNew` passes the list it just grew). */
  const send = useCallback(
    async (keys: string[], source: ReviewDraft[] = drafts) => {
      if (sendDisabledReason) {
        showToast(sendDisabledReason, true);
        return;
      }
      const targets = source.filter((d) => keys.includes(d.key));
      if (targets.length === 0) return;
      setSendingKeys(new Set(keys));
      const mustStart = !(runRef.current?.has_manager ?? false);
      setStartingManager(mustStart);
      if (mustStart) showToast("Manager not running — starting it before sending…");
      try {
        const res = await sendReviewComments(runId, toSendInputs(targets));
        persistDrafts(removeDrafts(source, keys));
        setEditingKey((k) => (k && keys.includes(k) ? null : k));
        try {
          const r = await fetchRun(runId);
          runRef.current = r;
          setRun(r);
        } catch {
          // The WebSocket refresh lands anyway.
        }
        showToast(
          `${plural(res.sent.length, "comment")} sent as one message${res.manager_started ? " — manager started" : ""}. Open the Manager tab to see it; replies show up inline.`,
        );
      } catch (e: unknown) {
        // Nothing is lost: the drafts stay drafts, the operator retries.
        showToast(`Send failed — ${e instanceof Error ? e.message : String(e)}. Your drafts are kept.`, true);
      } finally {
        setSendingKeys(new Set());
        setStartingManager(false);
      }
    },
    [drafts, runId, sendDisabledReason, showToast, persistDrafts],
  );

  const commentsApi = useMemo<ReviewCommentsApi>(
    () => ({
      editingKey,
      sendingKeys,
      startingManager,
      pairLabel,
      sendDisabledReason,
      saveNew: (anchor, text) => {
        persistDrafts(addDraft(drafts, anchor, pair, text));
        showToast("Draft saved — kept in this browser until you send it.");
      },
      sendNew: (anchor, text) => {
        const next = addDraft(drafts, anchor, pair, text);
        persistDrafts(next);
        const created = next[next.length - 1];
        void send([created.key], next);
      },
      editDraft: (key) => setEditingKey(key),
      updateDraft: (key, text) => {
        persistDrafts(updateDraft(drafts, key, text));
        setEditingKey(null);
      },
      cancelEdit: () => setEditingKey(null),
      deleteDraft: (key) => {
        persistDrafts(removeDrafts(drafts, [key]));
        setEditingKey((k) => (k === key ? null : k));
        showToast("Draft deleted.");
      },
      sendDraft: (key) => void send([key]),
      sentLineClicked: () =>
        showToast("This line already has a sent comment. The agent's replies show up under it; add a new comment on another line.", true),
      resolve: (c) => void decide(c, "resolve"),
      reopen: (c) => void decide(c, "reopen"),
      deciding,
      unreadOf: (c) => unreadReplies(c, seen),
      flashing,
      markSeen,
      showResolved,
      showOutdated,
      writtenOnLabel,
    }),
    [
      editingKey,
      sendingKeys,
      startingManager,
      pairLabel,
      sendDisabledReason,
      drafts,
      pair,
      persistDrafts,
      showToast,
      send,
      decide,
      deciding,
      seen,
      flashing,
      markSeen,
      showResolved,
      showOutdated,
      writtenOnLabel,
    ],
  );

  // --- Keyboard ---------------------------------------------------------------
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const t = e.target as HTMLElement | null;
      if (t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.isContentEditable)) {
        if (e.key === "Escape") t.blur();
        return;
      }
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      switch (e.key) {
        case "c":
          jumpComment(1);
          break;
        case "C":
          jumpComment(-1);
          break;
        case "j":
          goFile(Math.min(files.length - 1, currentIdx + 1));
          break;
        case "k":
          goFile(Math.max(0, currentIdx - 1));
          break;
        case "n":
          goHunk(1);
          break;
        case "p":
          goHunk(-1);
          break;
        case "u":
          changeView(view === "split" ? "unified" : "split");
          break;
        case "[":
          toggleList();
          break;
        case "/":
          e.preventDefault();
          if (!listOpen) toggleList();
          window.setTimeout(() => filterRef.current?.focus(), 0);
          break;
        default:
          return;
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [files.length, currentIdx, view, listOpen, goFile, goHunk, jumpComment]);

  // --- Exit -------------------------------------------------------------------
  const goBack = () => {
    const sameOrigin = document.referrer && document.referrer.startsWith(window.location.origin);
    if (sameOrigin && window.history.length > 1) window.history.back();
    else window.location.assign("/");
  };

  const reload = () => {
    setTipMoved(false);
    setNodeDelivered(null);
    setReloadTick((t) => t + 1);
  };

  const delivery = refs ? deliveryOfPair(pair, refs) : null;
  const stats = current.kind === "ready" ? current.diff : null;
  const pairIsDefault = isDefaultPair(pair, defaults);
  const defaultLabel = `${defaults.from} → ${defaults.to}`;
  const showsWorktree = pair.to === WORKTREE_ID;

  if (runError) {
    return (
      <div className="flex h-screen flex-col items-center justify-center gap-2 bg-bg-1 text-fg-3" data-testid="review-page">
        <div style={{ fontSize: "12px" }}>Run not found</div>
        <div className="text-fg-4" style={{ fontSize: "11px" }}>
          {runError}
        </div>
        <a href="/" className="mt-2 text-acc" style={{ fontSize: "11px" }}>
          Back to the app
        </a>
      </div>
    );
  }

  return (
    <div className="pdo-review grid h-screen grid-rows-[36px_1fr] bg-bg-1 text-fg" data-testid="review-page">
      {/* ===== Top bar ===== */}
      <div className="relative z-20 flex items-center gap-2 border-b border-line bg-bg-2 px-2.5" style={{ fontSize: "11px" }}>
        <button
          type="button"
          onClick={goBack}
          data-testid="review-back"
          title="Back to the Run (canvas stays where you left it)"
          className="flex cursor-pointer items-center gap-1.5 rounded px-1.5 py-[3px] text-fg-2 hover:bg-bg-3 hover:text-fg"
        >
          <ArrowLeft size={12} />
          <span className="text-fg-4">
            <span className="font-medium text-fg-2">{run?.pipeline_name ?? "…"}</span> · {runId}
          </span>
        </button>
        <span className="h-4 w-px bg-line-strong" />
        <span className="text-fg-3">Review</span>
        <span className="h-4 w-px bg-line-strong" />

        <div className="flex items-center gap-1" data-testid="review-refpair">
          <RefPicker
            side="from"
            value={pair.from}
            other={pair.to}
            refs={refs}
            disabled={isArchived}
            onPick={(id) => setPair({ from: id, to: pair.to })}
            onPickPair={setPair}
          />
          <span className="px-0.5 text-fg-4">→</span>
          <RefPicker
            side="to"
            value={pair.to}
            other={pair.from}
            refs={refs}
            disabled={isArchived}
            onPick={(id) => setPair({ from: pair.from, to: id })}
            onPickPair={setPair}
          />
          <button
            type="button"
            onClick={() => setPair({ from: pair.to, to: pair.from })}
            disabled={isArchived}
            title="Swap source and destination"
            data-testid="review-swap"
            className="cursor-pointer rounded p-[3px] text-fg-4 hover:bg-bg-3 hover:text-fg-2 disabled:cursor-not-allowed disabled:opacity-40"
          >
            <ArrowLeftRight size={11} />
          </button>
          {!pairIsDefault && (
            <button
              type="button"
              onClick={() => setPair(defaults)}
              title="Back to the default pair"
              data-testid="review-reset"
              className="cursor-pointer rounded px-1.5 py-0.5 text-fg-4 hover:bg-bg-3 hover:text-fg-2"
              style={{ fontSize: "10px" }}
            >
              {defaultLabel}
            </button>
          )}
        </div>
        {remapNote && (
          <span className="text-fg-3" style={{ fontSize: "10.5px" }} data-testid="review-remap-note" role="status">
            {remapNote}
          </span>
        )}

        <span className="flex-1" />

        {anyOutdated && (
          <button
            type="button"
            onClick={toggleShowOutdated}
            aria-pressed={showOutdated}
            title={showOutdated ? "Hide outdated comments (they stay counted)" : "Show outdated comments"}
            data-testid="review-toggle-outdated"
            className={`flex cursor-pointer items-center gap-1 rounded border border-line-strong px-2 py-0.5 ${
              showOutdated ? "bg-bg-3 text-fg-3 hover:text-fg-2" : "bg-bg-4 text-fg"
            }`}
            style={{ fontSize: "10.5px" }}
          >
            <History size={11} className="text-st-stale" /> Show outdated
          </button>
        )}
        {stats && (
          <span className="flex items-center gap-1.5 text-fg-2" data-testid="review-stats">
            <span>
              {stats.files_changed} file{stats.files_changed === 1 ? "" : "s"}
            </span>
            <span className="font-mono text-st-done">+{stats.additions}</span>
            <span className="font-mono text-st-failed">−{stats.deletions}</span>
          </span>
        )}
        <span className="h-4 w-px bg-line-strong" />
        <div
          className="inline-flex overflow-hidden rounded border border-line-strong"
          title={narrow ? "Unified on narrow windows; your preference is kept" : "Remembered on this browser"}
          role="group"
          data-testid="review-view-toggle"
          data-view={effectiveView}
        >
          <button
            type="button"
            onClick={() => changeView("split")}
            aria-pressed={effectiveView === "split"}
            data-testid="review-view-split"
            className={`flex cursor-pointer items-center gap-1 px-2 py-0.5 ${
              effectiveView === "split" ? "bg-bg-4 text-fg" : "text-fg-3 hover:text-fg-2"
            }`}
            style={{ fontSize: "10.5px" }}
          >
            <Columns2 size={11} /> Split
          </button>
          <button
            type="button"
            onClick={() => changeView("unified")}
            aria-pressed={effectiveView === "unified"}
            data-testid="review-view-unified"
            className={`flex cursor-pointer items-center gap-1 border-l border-line-strong px-2 py-0.5 ${
              effectiveView === "unified" ? "bg-bg-4 text-fg" : "text-fg-3 hover:text-fg-2"
            }`}
            style={{ fontSize: "10.5px" }}
          >
            <Rows3 size={11} /> Unified
          </button>
        </div>
        <button
          type="button"
          onClick={collapseAll}
          disabled={files.length === 0}
          title="Collapse all / expand all"
          data-testid="review-collapse-all"
          className="flex cursor-pointer items-center gap-1 rounded border border-line-strong bg-bg-3 px-2 py-0.5 text-fg-3 hover:text-fg-2 disabled:cursor-not-allowed disabled:opacity-50"
          style={{ fontSize: "10.5px" }}
        >
          <ListMinus size={11} /> Collapse all
        </button>
        {/* #750: pending drafts → the send-all gesture (also in the footer bar); otherwise a counter + navigator. */}
        {pairDrafts.length > 0 ? (
          <>
            <button
              type="button"
              onClick={() => void send(pairDrafts.map((d) => d.key))}
              disabled={!!sendDisabledReason || sendingKeys.size > 0}
              title={sendDisabledReason ?? "Send every draft to the manager as one message"}
              data-testid="review-send-pill"
              className="flex cursor-pointer items-center gap-1 rounded border border-acc-border bg-acc-bg px-2 py-0.5 text-acc hover:bg-acc/20 disabled:cursor-not-allowed disabled:opacity-45"
              style={{ fontSize: "10.5px" }}
            >
              ↗ Send {plural(pairDrafts.length, "draft")} to manager
            </button>
            {sentCount > 0 && (
              <span
                className="flex items-center gap-1 rounded border border-line-strong bg-bg-3 px-2 py-0.5 text-st-running"
                title={`${sentCount} sent, awaiting reply`}
                data-testid="review-sent-pill"
                style={{ fontSize: "10.5px" }}
              >
                ↗ {sentCount} sent
              </span>
            )}
          </>
        ) : (
          <>
            {unreadComments.length > 0 && (
              // The one filled pill: unread replies, click to jump to the first.
              <button
                type="button"
                onClick={jumpUnread}
                title={`${plural(unreadComments.length, "comment")} with an unread reply — jump to the first`}
                data-testid="review-unread-pill"
                className="flex cursor-pointer items-center gap-1 rounded border border-st-running bg-st-running px-2 py-0.5 font-semibold text-white hover:opacity-90"
                style={{ fontSize: "10.5px" }}
              >
                <Bell size={11} /> {unreadComments.length}
              </button>
            )}
            {sentCount === 0 ? (
              <button
                type="button"
                onClick={() => jumpComment(1)}
                disabled={cycleEntries.length === 0}
                title="Jump to the next comment  ( c / C )"
                data-testid="review-comments-pill"
                className="flex cursor-pointer items-center gap-1 rounded border border-line-strong bg-bg-3 px-2 py-0.5 text-fg-3 hover:text-fg-2 disabled:cursor-not-allowed disabled:opacity-50"
                style={{ fontSize: "10.5px" }}
              >
                <MessageSquare size={11} /> 0 comments
              </button>
            ) : (
              <span className="flex items-center gap-1" data-testid="review-state-pills">
                {pairStates.open > 0 && (
                  <button
                    type="button"
                    onClick={() => jumpComment(1)}
                    title={`${plural(pairStates.open, "open comment")} — jump to the next  ( c / C )`}
                    data-testid="review-comments-pill"
                    className="flex cursor-pointer items-center gap-1 rounded border border-line-strong bg-bg-3 px-2 py-0.5 text-fg-3 hover:text-fg-2"
                    style={{ fontSize: "10.5px" }}
                  >
                    <StateIcon state="open" size={11} /> {pairStates.open}
                  </button>
                )}
                {pairStates.proposed > 0 && (
                  <button
                    type="button"
                    onClick={() => jumpComment(1)}
                    title={`${pairStates.proposed} resolution${pairStates.proposed === 1 ? "" : "s"} proposed — your call`}
                    data-testid="review-proposed-pill"
                    className="flex cursor-pointer items-center gap-1 rounded border border-line-strong bg-bg-3 px-2 py-0.5 text-fg-3 hover:text-fg-2"
                    style={{ fontSize: "10.5px" }}
                  >
                    <StateIcon state="proposed" size={11} /> {pairStates.proposed}
                  </button>
                )}
                {pairStates.resolved > 0 && (
                  <span
                    title={`${pairStates.resolved} resolved (skipped by c / C)`}
                    data-testid="review-resolved-pill"
                    className="flex items-center gap-1 rounded border border-line-strong bg-bg-3 px-2 py-0.5 text-fg-3"
                    style={{ fontSize: "10.5px" }}
                  >
                    <StateIcon state="resolved" size={11} /> {pairStates.resolved}
                  </span>
                )}
                <button
                  type="button"
                  onClick={toggleShowResolved}
                  aria-pressed={!showResolved}
                  title={showResolved ? "Hide resolved comments" : "Show resolved comments"}
                  data-testid="review-toggle-resolved"
                  className={`grid h-5 w-[22px] cursor-pointer place-items-center rounded border border-line-strong ${
                    showResolved ? "bg-bg-3 text-fg-3 hover:text-fg-2" : "bg-bg-4 text-fg"
                  }`}
                >
                  {showResolved ? <Eye size={11} /> : <EyeOff size={11} />}
                </button>
              </span>
            )}
          </>
        )}
        <a
          href={reviewUrl(runId, pair, defaults)}
          target="_blank"
          rel="noreferrer"
          title="Open in a new tab — the URL carries the pair"
          data-testid="review-open-new-tab"
          className="grid h-5 w-[22px] place-items-center rounded border border-line-strong bg-bg-3 text-fg-3 hover:text-fg-2"
        >
          <SquareArrowOutUpRight size={11} />
        </a>
      </div>

      {/* ===== Body ===== */}
      <div className={`grid min-h-0 ${listOpen ? "grid-cols-[272px_1fr]" : "grid-cols-[0_1fr]"}`} data-testid="review-body">
        {listOpen ? (
          <ReviewFileList
            files={files}
            filter={filter}
            onFilterChange={setFilter}
            filterRef={filterRef}
            currentIndex={currentIdx}
            onSelect={goFile}
            onClose={toggleList}
            counts={counts}
            stateCounts={states}
            outdatedCounts={outdatedCounts}
            comments={{ current: entries, other: otherEntries.map(({ entry, pair: p }) => ({ entry, pair: p })) }}
            onJumpEntry={(e) => jumpTo(e.anchor)}
            onJumpOther={(e) => {
              const o = otherEntries.find((x) => x.entry === e);
              if (!o) return;
              setPair(o.refPair);
              window.setTimeout(() => jumpTo(e.anchor), 400);
            }}
          />
        ) : (
          <div />
        )}

        <div ref={mainRef} onScroll={onScroll} className="relative min-h-0 min-w-0 overflow-y-auto" data-testid="review-main">
          {!listOpen && (
            <button
              type="button"
              onClick={toggleList}
              title="Show file list  ( [ )"
              data-testid="review-list-open"
              className="absolute left-2 top-2 z-20 grid h-6 w-6 cursor-pointer place-items-center rounded border border-line-strong bg-bg-2 text-fg-4 hover:text-fg-2"
            >
              <PanelLeftOpen size={12} />
            </button>
          )}

          {tipMoved && !isArchived && (
            <div
              className="sticky top-0 z-[15] flex items-center gap-2.5 border-b border-st-running/35 bg-st-running-bg px-3 py-1.5 text-st-running"
              style={{ fontSize: "11px" }}
              data-testid="review-tip-moved"
            >
              <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-current" />
              The Run tip moved since this diff was loaded ·
              <button
                type="button"
                onClick={reload}
                className="cursor-pointer rounded border border-st-running/50 px-2 py-px hover:bg-st-running/20"
              >
                Reload
              </button>
              <span className="text-fg-4">Your scroll position is kept.</span>
            </div>
          )}
          {showsWorktree && !tipMoved && !isArchived && run?.status === "running" && (
            <div
              className="flex items-center gap-2.5 border-b border-line bg-bg-2 px-3 py-1 text-fg-3"
              style={{ fontSize: "10.5px" }}
              data-testid="review-worktree-note"
            >
              <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-st-running" />
              Working tree as of load — commits and uncommitted edits of the running Run ·
              <button
                type="button"
                onClick={reload}
                className="cursor-pointer rounded border border-line-strong px-2 py-px hover:bg-bg-3"
              >
                Reload
              </button>
            </div>
          )}
          {nodeDelivered && refs && (
            <div
              className="sticky top-0 z-[15] flex items-center gap-2.5 border-b border-st-running/35 bg-st-running-bg px-3 py-1.5 text-st-running"
              style={{ fontSize: "11px" }}
              data-testid="review-node-delivered"
            >
              Node delivered · its live branch is gone ·
              <button
                type="button"
                onClick={() =>
                  setPair({
                    from: `node:${nodeDelivered.nodeId}:${nodeDelivered.iter}:before`,
                    to: `node:${nodeDelivered.nodeId}:${nodeDelivered.iter}:after`,
                  })
                }
                className="cursor-pointer rounded border border-st-running/50 px-2 py-px hover:bg-st-running/20"
              >
                Switch to its after ref
              </button>
            </div>
          )}

          {sendDisabledReason && !isArchived && (
            <div
              className="mx-3 mt-2.5 flex items-center gap-2 rounded-md border border-st-await/35 bg-st-await-bg px-2.5 py-1.5 text-fg-2"
              style={{ fontSize: "11px" }}
              data-testid="review-branch-gone"
            >
              ⚠ <span>{sendDisabledReason}</span>
            </div>
          )}

          {notice && (
            <div
              className="mx-3 mt-2.5 flex items-center gap-2 rounded-md border border-st-await/35 bg-st-await-bg px-2.5 py-1.5 text-fg-2"
              style={{ fontSize: "11px" }}
              data-testid="review-notice"
            >
              {notice}
              <button type="button" onClick={() => setNotice(null)} className="ml-auto cursor-pointer text-fg-3 hover:text-fg">
                ✕
              </button>
            </div>
          )}

          {delivery && !isArchived && (
            <div
              className="mx-3 mt-2.5 flex items-center gap-2 rounded-md border border-acc-border bg-acc-bg px-2.5 py-1.5 text-fg-2"
              style={{ fontSize: "11px" }}
              data-testid="review-delivery-chip"
            >
              ◈ Reviewing the delivery of <span className="font-medium text-acc">{delivery.node_name}</span> · iter{" "}
              {delivery.iter}
              {delivery.status === "running" && (
                <span className="text-fg-4">(live sub-worktree, not merged back yet)</span>
              )}
              <button
                type="button"
                onClick={() => setPair(defaults)}
                className="ml-auto cursor-pointer text-fg-3 hover:text-fg"
                style={{ fontSize: "10.5px" }}
                data-testid="review-whole-run"
              >
                Whole Run instead ({defaultLabel})
              </button>
            </div>
          )}

          {isArchived ? (
            <EmptyState
              testId="review-archived"
              icon={<Archive size={22} className="text-fg-5" />}
              title="Diff not preserved for archived runs"
              hint="The run branch was deleted at cleanup."
            />
          ) : sameRef ? (
            <EmptyState
              testId="review-empty"
              icon={<span className="text-fg-5" style={{ fontSize: "24px" }}>≡</span>}
              title="Nothing to compare"
              hint="Source and destination are the same ref. Pick another destination."
            />
          ) : current.kind === "none" ? (
            <div className="px-4 py-10 text-center text-fg-4" style={{ fontSize: "11px" }} data-testid="review-loading">
              Loading diff…
            </div>
          ) : current.kind === "error" ? (
            <div className="flex flex-col items-center gap-2 px-4 py-14 text-center" data-testid="review-error">
              <div className="text-fg-3" style={{ fontSize: "12px" }}>
                {current.status === 404 ? "Run branch not found" : "Could not load the diff"}
              </div>
              <div className="text-fg-4" style={{ fontSize: "11px" }}>
                {current.message}
              </div>
              <button
                type="button"
                onClick={reload}
                className="mt-1 cursor-pointer rounded border border-line-strong bg-bg-3 px-2 py-0.5 text-fg-2 hover:text-fg"
                style={{ fontSize: "10.5px" }}
              >
                Retry
              </button>
            </div>
          ) : files.length === 0 ? (
            <EmptyState
              testId="review-empty"
              icon={<span className="text-fg-5" style={{ fontSize: "24px" }}>≡</span>}
              title="No changes"
              hint="The two refs have identical trees."
            />
          ) : (
            <>
              {files.length > 40 && (
                <div className="mx-3 mt-2.5 rounded border border-line bg-bg-3 px-2.5 py-1 text-fg-3" style={{ fontSize: "10.5px" }}>
                  Large diff — files collapsed
                </div>
              )}
              {files.map((f) => {
                const p = filePath(f);
                return (
                  <ReviewFileCard
                    key={`${pair.from}|${pair.to}|${p}`}
                    runId={runId}
                    file={f}
                    pair={pair}
                    view={effectiveView}
                    collapsed={collapsedSet.has(p)}
                    onToggle={() => toggleFile(p)}
                    registerEl={registerEl}
                    entries={entriesByPath.get(p)}
                    comments={commentsApi}
                  />
                );
              })}
              <ReviewSendBar
                drafts={pairDrafts}
                sentCount={sentCount}
                sending={sendingKeys.size > 0}
                managerRunning={run?.has_manager ?? false}
                sendDisabledReason={sendDisabledReason}
                onJump={(d) => jumpTo(d)}
                onSendAll={() => void send(pairDrafts.map((d) => d.key))}
              />
              <div className="h-[40vh]" />
            </>
          )}
        </div>
      </div>

      {toast && (
        <div
          role="status"
          data-testid="review-toast"
          data-error={toast.error ? "true" : undefined}
          className={`fixed right-3.5 z-[60] max-w-[420px] rounded-md border border-line-strong bg-bg-3 px-2.5 py-[7px] text-fg-2 shadow-[0_8px_30px_rgba(0,0,0,.5)] ${
            pairDrafts.length > 0 ? "bottom-[52px]" : "bottom-3.5"
          } ${
            toast.error ? "border-l-[3px] border-l-st-failed" : "border-l-[3px] border-l-acc"
          }`}
          style={{ fontSize: "11px" }}
        >
          {toast.text}
        </div>
      )}
    </div>
  );
}

function EmptyState({
  testId,
  icon,
  title,
  hint,
}: {
  testId: string;
  icon: React.ReactNode;
  title: string;
  hint: string;
}) {
  return (
    <div className="flex flex-col items-center gap-1 px-4 pt-20 text-center" data-testid={testId}>
      <div className="mb-1">{icon}</div>
      <div className="text-fg-3" style={{ fontSize: "12px" }}>
        {title}
      </div>
      <div className="text-fg-4" style={{ fontSize: "11px" }}>
        {hint}
      </div>
    </div>
  );
}
