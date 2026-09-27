import { useCallback, useEffect, useMemo } from "react";
import { PanelLeftOpen } from "lucide-react";
import "@git-diff-view/react/styles/diff-view.css";
import { filePath } from "../../run/model/runRefs";
import type { RefPair } from "../../run/model/runRefs";
import { sendDisabledReason as sendReasonOf } from "../model/commentStatus";
import { useReviewCommentState } from "../store/useReviewCommentState";
import { useReviewDiff } from "../store/useReviewDiff";
import { useReviewEntries } from "../store/useReviewEntries";
import { useReviewGestures } from "../store/useReviewGestures";
import { useReviewLayout } from "../store/useReviewLayout";
import { useReviewLive } from "../store/useReviewLive";
import { useReviewRun } from "../store/useReviewRun";
import ReviewBanners from "./ReviewBanners";
import ReviewDiffBody from "./ReviewDiffBody";
import ReviewFileList from "./ReviewFileList";
import ReviewTopBar from "./ReviewTopBar";

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
 *
 * The page composes the review's stores: the Run and its pair
 * (`useReviewRun`), the diff and its re-map (`useReviewDiff`), the Run's live
 * events (`useReviewLive`), the layout (`useReviewLayout`), the comments'
 * state, list and gestures (`useReviewCommentState`, `useReviewEntries`,
 * `useReviewGestures`).
 */

interface Props {
  runId: string;
}

export default function ReviewPage({ runId }: Props) {
  const r = useReviewRun(runId);
  const { run, refs, pair, defaults } = r;
  const d = useReviewDiff({
    runId,
    run,
    runRef: r.runRef,
    sigAtLoad: r.sigAtLoad,
    pair,
    defaults,
    reloadTick: r.reloadTick,
    refsSettled: r.refsSettled,
  });
  const { setTipMoved } = d;
  const onTipMoved = useCallback(() => setTipMoved(true), [setTipMoved]);
  const live = useReviewLive({
    runId,
    runRef: r.runRef,
    setRun: r.setRun,
    sigAtLoad: r.sigAtLoad,
    pairRef: r.pairRef,
    onTipMoved,
  });

  const { shown } = d;
  const files = useMemo(() => (shown.kind === "ready" ? shown.diff.files : []), [shown]);
  const {
    view,
    narrow,
    effectiveView,
    changeView,
    listOpen,
    toggleList,
    filter,
    setFilter,
    filterRef,
    mainRef,
    collapsedSet,
    toggleFile,
    collapseAll,
    resetForPair,
    scrollTop,
    currentIdx,
    registerEl,
    onScroll,
    goFile,
    goHunk,
    jumpTo,
  } = useReviewLayout(files);
  const { choosePair } = r;
  const { setNodeDelivered } = live;
  const setPair = useCallback(
    (next: RefPair) => {
      choosePair(next);
      resetForPair();
      setNodeDelivered(null);
      scrollTop();
    },
    // `resetForPair` / `scrollTop` only touch setters and a ref.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [choosePair, setNodeDelivered],
  );

  const c = useReviewCommentState(runId);
  const pathOrder = useMemo(() => files.map(filePath), [files]);
  const { pathOrderRef } = d;
  useEffect(() => {
    pathOrderRef.current = pathOrder;
  }, [pathOrder, pathOrderRef]);
  const entries = useReviewEntries({
    run,
    refs,
    pair,
    drafts: c.drafts,
    pathOrder,
    mappings: d.activeMappings,
    seen: c.seen,
  });
  const sendDisabledReason = sendReasonOf(run, refs);
  const g = useReviewGestures({
    runId,
    runRef: r.runRef,
    refreshRun: r.refreshRun,
    pair,
    state: c,
    entries,
    flashing: live.flashing,
    sendDisabledReason,
    jumpTo,
  });

  // --- Keyboard ---------------------------------------------------------------
  const { jumpComment } = g;
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
    // The view and list toggles are re-created every render; they read state.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [files.length, currentIdx, view, listOpen, goFile, goHunk, jumpComment]);

  const reload = () => {
    setTipMoved(false);
    setNodeDelivered(null);
    r.reload();
  };

  if (r.runError) {
    return (
      <div className="flex h-screen flex-col items-center justify-center gap-2 bg-bg-1 text-fg-3" data-testid="review-page">
        <div style={{ fontSize: "12px" }}>Run not found</div>
        <div className="text-fg-4" style={{ fontSize: "11px" }}>
          {r.runError}
        </div>
        <a href="/" className="mt-2 text-acc" style={{ fontSize: "11px" }}>
          Back to the app
        </a>
      </div>
    );
  }

  const { toast } = c;
  const { pairDrafts, sentCount } = entries;
  return (
    <div className="pdo-review grid h-screen grid-rows-[36px_1fr] bg-bg-1 text-fg" data-testid="review-page">
      <ReviewTopBar
        runId={runId}
        run={run}
        refs={refs}
        pair={pair}
        setPair={setPair}
        defaults={defaults}
        isArchived={d.isArchived}
        remapNote={d.remapNote}
        anyOutdated={entries.anyOutdated}
        showOutdated={c.showOutdated}
        toggleShowOutdated={c.toggleShowOutdated}
        stats={shown.kind === "ready" ? shown.diff : null}
        narrow={narrow}
        effectiveView={effectiveView}
        changeView={changeView}
        collapseAll={collapseAll}
        filesCount={files.length}
        pairDrafts={pairDrafts}
        send={g.send}
        sendDisabledReason={sendDisabledReason}
        sendingKeys={c.sendingKeys}
        sentCount={sentCount}
        unreadComments={entries.unreadComments}
        jumpUnread={g.jumpUnread}
        jumpComment={jumpComment}
        cycleEntries={entries.cycleEntries}
        pairStates={entries.pairStates}
        toggleShowResolved={c.toggleShowResolved}
        showResolved={c.showResolved}
      />

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
            counts={entries.counts}
            stateCounts={entries.states}
            outdatedCounts={entries.outdatedCounts}
            comments={{ current: entries.entries, other: entries.otherEntries.map(({ entry, pair: p }) => ({ entry, pair: p })) }}
            onJumpEntry={(e) => jumpTo(e.anchor)}
            onJumpOther={(e) => {
              const o = entries.otherEntries.find((x) => x.entry === e);
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
          <ReviewBanners
            run={run}
            refs={refs}
            pair={pair}
            setPair={setPair}
            defaults={defaults}
            isArchived={d.isArchived}
            tipMoved={d.tipMoved}
            nodeDelivered={live.nodeDelivered}
            reload={reload}
            sendDisabledReason={sendDisabledReason}
            notice={r.notice}
            setNotice={r.setNotice}
          />
          <ReviewDiffBody
            runId={runId}
            pair={pair}
            isArchived={d.isArchived}
            sameRef={d.sameRef}
            current={shown}
            reload={reload}
            files={files}
            effectiveView={effectiveView}
            collapsedSet={collapsedSet}
            toggleFile={toggleFile}
            registerEl={registerEl}
            entriesByPath={entries.entriesByPath}
            commentsApi={g.commentsApi}
            pairDrafts={pairDrafts}
            sentCount={sentCount}
            sendingKeys={c.sendingKeys}
            managerRunning={run?.has_manager ?? false}
            sendDisabledReason={sendDisabledReason}
            jumpTo={jumpTo}
            send={g.send}
          />
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
