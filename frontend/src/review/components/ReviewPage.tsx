import { useCallback, useEffect, useMemo, type ReactNode } from "react";
import type { RunState } from "../../shared/generated/RunState";
import "@git-diff-view/react/styles/diff-view.css";
import { filePath } from "../../run/model/runRefs";
import type { RefPair } from "../../run/model/runRefs";
import { sendDisabledReason as sendReasonOf } from "../model/commentStatus";
import { treeOrder } from "../model/fileTree";
import { useReviewCommentState } from "../store/useReviewCommentState";
import { useReviewDiff } from "../store/useReviewDiff";
import { useReviewEntries } from "../store/useReviewEntries";
import { useReviewGestures } from "../store/useReviewGestures";
import { useReviewRefusals } from "../store/useReviewRefusals";
import { useReviewKeyboard } from "../store/useReviewKeyboard";
import { useReviewLayout } from "../store/useReviewLayout";
import { useReviewFileLanding } from "../store/useReviewFileLanding";
import { useReviewLive } from "../store/useReviewLive";
import { useReviewRun } from "../store/useReviewRun";
import { useReviewViewed } from "../store/useReviewViewed";
import ReviewBanners from "./ReviewBanners";
import ReviewDiffBody from "./ReviewDiffBody";
import ReviewFileList from "./ReviewFileList";
import ReviewTopBar from "./ReviewTopBar";
import { useProjects, useRuns } from "../../app/useAppData";
import ExtensionContextProvider from "../../shared/extensions/ExtensionContextProvider";
import Toaster from "../../shared/ui/Toaster";

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
  // CORE-84: the cards follow the list's folder tree.
  const files = useMemo(() => (shown.kind === "ready" ? treeOrder(shown.diff.files, filePath) : []), [shown]);
  // « Viewed » (the forge's checkbox): a viewed file opens folded; marking a
  // file folds its card, unmarking it unfolds it again.
  const viewedState = useReviewViewed(runId, pair);
  const layout = useReviewLayout(files, viewedState.viewed);
  const {
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
    landOnFile,
    jumpTo,
    rows,
    toggleDir,
    toggleAllDirs,
    allDirsCollapsed,
    hasDirs,
  } = layout;
  useReviewFileLanding(files, landOnFile);
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
  // CORE-205: Send, Resolve and Reopen the caller may not make stay, greyed.
  const commentIds = useMemo(() => (run?.review_comments ?? []).map((cm) => cm.id), [run?.review_comments]);
  const refusals = useReviewRefusals(runId, commentIds);
  const g = useReviewGestures({
    runId,
    runRef: r.runRef,
    refreshRun: r.refreshRun,
    pair,
    state: c,
    entries,
    flashing: live.flashing,
    sendDisabledReason,
    refusals,
    jumpTo,
  });

  const { jumpComment } = g;
  const toggleViewed = (path: string) => {
    const marking = !viewedState.viewed.has(path);
    viewedState.toggle(path);
    if (marking !== collapsedSet.has(path)) toggleFile(path);
  };
  const viewedCount = files.filter((f) => viewedState.viewed.has(filePath(f))).length;
  useReviewKeyboard(layout, jumpComment, () => {
    const current = files[currentIdx];
    if (current) toggleViewed(filePath(current));
  });

  const reload = () => {
    setTipMoved(false);
    setNodeDelivered(null);
    r.reload();
  };

  if (r.runError) {
    return (
      <div className="flex h-[calc(100dvh/var(--app-zoom,1))] flex-col items-center justify-center gap-2 bg-surface-0 text-text-3" data-testid="review-page">
        <div className="text-md text-text-2">Run not found</div>
        <div className="text-sm">{r.runError}</div>
        <a href="/" className="d-btn mt-2">
          Back to the app
        </a>
      </div>
    );
  }

  const { toast } = c;
  const { pairDrafts, sentCount } = entries;
  return (
    <ReviewExtensions run={run}>
    <div className="pdo-review h-[calc(100dvh/var(--app-zoom,1))] bg-surface-0 text-text" data-testid="review-page">
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
        sentCount={sentCount}
        unreadComments={entries.unreadComments}
        jumpUnread={g.jumpUnread}
        jumpComment={jumpComment}
        cycleEntries={entries.cycleEntries}
        pairStates={entries.pairStates}
        toggleShowResolved={c.toggleShowResolved}
        showResolved={c.showResolved}
        listOpen={listOpen}
        toggleList={toggleList}
        viewedCount={viewedCount}
      />

      <div className={`rv-body ${listOpen ? "grid-cols-[300px_1fr]" : "grid-cols-[0_1fr]"}`} data-testid="review-body">
        {listOpen ? (
          <ReviewFileList
            files={files}
            rows={rows}
            onToggleDir={toggleDir}
            onToggleAllDirs={hasDirs ? toggleAllDirs : undefined}
            allDirsCollapsed={allDirsCollapsed}
            filter={filter}
            onFilterChange={setFilter}
            filterRef={filterRef}
            currentIndex={currentIdx}
            onSelect={goFile}
            onClose={toggleList}
            viewed={viewedState.viewed}
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

        <div ref={mainRef} onScroll={onScroll} className="rv-main" data-testid="review-main">
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
            viewed={viewedState.viewed}
            toggleViewed={toggleViewed}
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
          className={`rv-toast ${pairDrafts.length > 0 ? "bottom-[52px]" : "bottom-3.5"}`}
        >
          {toast.text}
        </div>
      )}
    </div>
    </ReviewExtensions>
  );
}

/**
 * What the extensions' actions of the Review page (#30) need around them: the
 * page's Run as the selected Run, with its Projet (the Runs list's rule, hence
 * the Runs and the Projets), the tooltips of their buttons and the toasts of
 * their commands.
 */
function ReviewExtensions({ run, children }: { run: RunState | null; children: ReactNode }) {
  const { runs, refresh: refreshRuns } = useRuns();
  const { projects, refresh: refreshProjects } = useProjects();
  useEffect(() => {
    void refreshRuns();
    void refreshProjects();
  }, [refreshRuns, refreshProjects]);
  return (
    <>
      <ExtensionContextProvider selectedRun={run} runs={runs} projects={projects}>
        {children}
        <Toaster />
      </ExtensionContextProvider>
    </>
  );
}
