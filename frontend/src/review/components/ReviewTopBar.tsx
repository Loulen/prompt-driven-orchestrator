import {
  ArrowLeftRight,
  ChevronsDownUp,
  Columns2,
  FileDiff,
  GitBranch,
  History,
  MessageSquare,
  PanelLeft,
  Rows3,
  SquareArrowOutUpRight,
  X,
} from "lucide-react";
import { IconButton } from "../../shared/ui/kit";
import { ReviewIcon } from "../../shared/ui/brand/AccentIcons";
import { isDefaultPair, reviewUrl } from "../../run/model/runRefs";
import { useMemo } from "react";
import { ExtensionActionButtons } from "../../shared/extensions/ExtensionActions";
import RefPicker from "./RefPicker";
import ReviewCommentPills from "./ReviewCommentPills";
import type { ReviewComment } from "../../shared/generated/ReviewComment";
import type { RunRefs } from "../../shared/generated/RunRefs";
import type { RunState } from "../../shared/generated/RunState";
import type { StructuredDiff } from "../../shared/generated/StructuredDiff";
import type { RefPair, ViewMode } from "../../run/model/runRefs";
import type { StateCounts } from "../model/commentStatus";
import type { ReviewDraft } from "../model/drafts";
import type { ReviewEntry } from "../model/entries";

interface Props {
  runId: string;
  run: RunState | null;
  refs: RunRefs | null;
  pair: RefPair;
  setPair: (next: RefPair) => void;
  defaults: RefPair;
  isArchived: boolean;
  remapNote: string | null;
  anyOutdated: boolean;
  showOutdated: boolean;
  toggleShowOutdated: () => void;
  stats: StructuredDiff | null;
  narrow: boolean;
  effectiveView: ViewMode;
  changeView: (v: ViewMode) => void;
  collapseAll: () => void;
  filesCount: number;
  pairDrafts: ReviewDraft[];
  sentCount: number;
  unreadComments: ReviewComment[];
  jumpUnread: () => void;
  jumpComment: (dir: 1 | -1) => void;
  cycleEntries: ReviewEntry[];
  pairStates: StateCounts;
  toggleShowResolved: () => void;
  showResolved: boolean;
  /** The file list: shown, and its toggle. */
  listOpen: boolean;
  toggleList: () => void;
  /** Files marked « Viewed » for this pair. */
  viewedCount: number;
}

/**
 * The top of the Review page (#749), two rows. The header: the Run (back to
 * it), its source branch, the stats of the pair, the comments open / total,
 * the comment pills and the close. The bar: the file list toggle, the
 * `source → destination` pair (swap, back to the default), the re-map note
 * (#752), the outdated toggle, the files viewed, split / unified, collapse
 * all, the extensions' actions and "open in a new tab". Neither row wraps:
 * the Run label and the re-map note truncate first.
 */
export default function ReviewTopBar({
  runId,
  run,
  refs,
  pair,
  setPair,
  defaults,
  isArchived,
  remapNote,
  anyOutdated,
  showOutdated,
  toggleShowOutdated,
  stats,
  narrow,
  effectiveView,
  changeView,
  collapseAll,
  filesCount,
  pairDrafts,
  sentCount,
  unreadComments,
  jumpUnread,
  jumpComment,
  cycleEntries,
  pairStates,
  toggleShowResolved,
  showResolved,
  listOpen,
  toggleList,
  viewedCount,
}: Props) {
  const extensionExtra = useMemo(() => ({ review: { from: pair.from, to: pair.to } }), [pair.from, pair.to]);
  const pairIsDefault = isDefaultPair(pair, defaults);
  const defaultLabel = `${defaults.from} → ${defaults.to}`;
  const comments = run?.review_comments ?? [];
  const openComments = pairStates.open + pairStates.proposed;
  return (
    <>
      <header className="rv-head">
        <h1 className="rv-head__title">
          <ReviewIcon size={16} strokeWidth={1.5} aria-hidden />
          Review
        </h1>
        <button
          type="button"
          onClick={goBack}
          data-testid="review-back"
          title={`Back to the Run · ${run?.pipeline_name ?? ""} · ${runId}`}
          className="rv-head__run"
        >
          {run?.name && <span className="text-text">{run.name}</span>}
          <span>
            {run?.pipeline_name ?? "…"} · {runId}
          </span>
        </button>
        {run?.source_branch && (
          <span className="rv-fact" title="Source branch">
            <GitBranch size={12} strokeWidth={1.5} aria-hidden />
            {run.source_branch}
          </span>
        )}
        {stats && (
          <span className="rv-fact" data-testid="review-stats">
            <FileDiff size={12} strokeWidth={1.5} aria-hidden />
            <span>
              {stats.files_changed} file{stats.files_changed === 1 ? "" : "s"}
            </span>
            <span className="rv-add">+{stats.additions}</span>
            <span className="rv-del">−{stats.deletions}</span>
          </span>
        )}
        {comments.length > 0 && (
          <span
            className="rv-fact"
            title={`${comments.length} comment${comments.length === 1 ? "" : "s"}, ${openComments} open on this pair`}
            data-testid="review-comment-total"
          >
            <MessageSquare size={12} strokeWidth={1.5} aria-hidden />
            {openComments}/{comments.length}
          </span>
        )}
        <span className="rv-grow" />
        <ReviewCommentPills
          pairDrafts={pairDrafts}
          sentCount={sentCount}
          unreadComments={unreadComments}
          jumpUnread={jumpUnread}
          jumpComment={jumpComment}
          cycleEntries={cycleEntries}
          pairStates={pairStates}
          toggleShowResolved={toggleShowResolved}
          showResolved={showResolved}
        />
        <span className="rv-vsep" aria-hidden />
        <IconButton icon={X} label="Back to the app" onClick={goBack} data-testid="review-close" />
      </header>

      <div className="rv-bar" data-testid="review-bar">
        <IconButton
          icon={PanelLeft}
          label={listOpen ? "Hide file list" : "Show file list"}
          kbd="["
          pressed={listOpen}
          onClick={toggleList}
          data-testid={listOpen ? "review-list-toggle" : "review-list-open"}
        />
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
          <IconButton
            icon={ArrowLeftRight}
            label="Swap source and destination"
            onClick={() => setPair({ from: pair.to, to: pair.from })}
            disabled={isArchived}
            data-testid="review-swap"
          />
          <RefPicker
            side="to"
            value={pair.to}
            other={pair.from}
            refs={refs}
            disabled={isArchived}
            onPick={(id) => setPair({ from: pair.from, to: id })}
            onPickPair={setPair}
          />
          {!pairIsDefault && (
            <button
              type="button"
              onClick={() => setPair(defaults)}
              title="Back to the default pair"
              data-testid="review-reset"
              className="d-btn d-btn--ghost d-btn--sm font-mono"
            >
              {defaultLabel}
            </button>
          )}
        </div>
        {remapNote && (
          <span className="rv-note" title={remapNote} data-testid="review-remap-note" role="status">
            {remapNote}
          </span>
        )}

        <span className="rv-grow" />

        {anyOutdated && (
          <button
            type="button"
            onClick={toggleShowOutdated}
            aria-pressed={showOutdated}
            title={showOutdated ? "Hide outdated comments (they stay counted)" : "Show outdated comments"}
            data-testid="review-toggle-outdated"
            className="rv-pill"
          >
            <History size={12} strokeWidth={1.5} className="text-st-skipped" aria-hidden /> Show outdated
          </button>
        )}
        {filesCount > 0 && (
          <span className="rv-bar__meta" title="Files marked Viewed in this browser" data-testid="review-viewed-count">
            {viewedCount}/{filesCount} viewed
          </span>
        )}
        <div
          className="d-seg d-seg--sm rv-seg"
          title={narrow ? "Unified on narrow windows; your preference is kept" : "Remembered on this browser"}
          role="group"
          aria-label="View"
          data-testid="review-view-toggle"
          data-view={effectiveView}
        >
          <button
            type="button"
            onClick={() => changeView("split")}
            aria-pressed={effectiveView === "split"}
            data-testid="review-view-split"
            className="d-seg__btn"
          >
            <Columns2 size={12} strokeWidth={1.5} aria-hidden /> Split
          </button>
          <button
            type="button"
            onClick={() => changeView("unified")}
            aria-pressed={effectiveView === "unified"}
            data-testid="review-view-unified"
            className="d-seg__btn"
          >
            <Rows3 size={12} strokeWidth={1.5} aria-hidden /> Unified
          </button>
        </div>
        <IconButton
          icon={ChevronsDownUp}
          label="Collapse all / expand all"
          onClick={collapseAll}
          disabled={filesCount === 0}
          data-testid="review-collapse-all"
        />
        {/* #30 — `review/toolbar`: the extensions' icon buttons, before « open in a new tab ». */}
        <ExtensionActionButtons
          location="review/toolbar"
          extra={extensionExtra}
          className="flex shrink-0 items-center gap-1"
          buttonClassName="d-icon"
          iconSize={14}
        />
        <a
          href={reviewUrl(runId, pair, defaults)}
          target="_blank"
          rel="noreferrer"
          title="Open in a new tab — the URL carries the pair"
          aria-label="Open in a new tab"
          data-testid="review-open-new-tab"
          className="d-icon"
        >
          <SquareArrowOutUpRight size={14} strokeWidth={1.5} aria-hidden />
        </a>
      </div>
    </>
  );
}

function goBack() {
  const sameOrigin = document.referrer && document.referrer.startsWith(window.location.origin);
  if (sameOrigin && window.history.length > 1) window.history.back();
  else window.location.assign("/");
}
