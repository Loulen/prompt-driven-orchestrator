import { ArrowLeftRight, ArrowLeft, Columns2, History, ListMinus, Rows3, SquareArrowOutUpRight } from "lucide-react";
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
  send: (keys: string[]) => Promise<void>;
  sendDisabledReason: string | null;
  sendingKeys: ReadonlySet<string>;
  sentCount: number;
  unreadComments: ReviewComment[];
  jumpUnread: () => void;
  jumpComment: (dir: 1 | -1) => void;
  cycleEntries: ReviewEntry[];
  pairStates: StateCounts;
  toggleShowResolved: () => void;
  showResolved: boolean;
}

/**
 * The top bar of the Review page (#749): back to the Run, the `source →
 * destination` pair (swap, back to the default), the re-map note (#752), the
 * outdated toggle, the stats, split / unified, collapse all, the comment pills
 * and "open in a new tab". One line whatever the width: the buttons and pills
 * never wrap, the pipeline · run label truncates first.
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
  send,
  sendDisabledReason,
  sendingKeys,
  sentCount,
  unreadComments,
  jumpUnread,
  jumpComment,
  cycleEntries,
  pairStates,
  toggleShowResolved,
  showResolved,
}: Props) {
  const extensionExtra = useMemo(() => ({ review: { from: pair.from, to: pair.to } }), [pair.from, pair.to]);
  const pairIsDefault = isDefaultPair(pair, defaults);
  const defaultLabel = `${defaults.from} → ${defaults.to}`;
  return (
    <div className="relative z-20 flex min-w-0 items-center gap-2 whitespace-nowrap border-b border-line bg-surface-1 px-2.5" style={{ fontSize: "11px" }}>
      <button
        type="button"
        onClick={goBack}
        data-testid="review-back"
        title="Back to the Run (canvas stays where you left it)"
        className="flex min-w-0 cursor-pointer items-center gap-1.5 rounded px-1.5 py-[3px] text-text-2 hover:bg-hover hover:text-text"
      >
        <ArrowLeft size={12} className="shrink-0" />
        <span className="truncate text-text-3">
          <span className="font-medium text-text-2">{run?.pipeline_name ?? "…"}</span> · {runId}
        </span>
      </button>
      <span className="h-4 w-px bg-line-strong" />
      <span className="text-text-3">Review</span>
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
        <span className="px-0.5 text-text-3">→</span>
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
          className="cursor-pointer rounded p-[3px] text-text-3 hover:bg-hover hover:text-text-2 disabled:cursor-not-allowed disabled:opacity-40"
        >
          <ArrowLeftRight size={11} />
        </button>
        {!pairIsDefault && (
          <button
            type="button"
            onClick={() => setPair(defaults)}
            title="Back to the default pair"
            data-testid="review-reset"
            className="cursor-pointer rounded px-1.5 py-0.5 text-text-3 hover:bg-hover hover:text-text-2"
            style={{ fontSize: "10px" }}
          >
            {defaultLabel}
          </button>
        )}
      </div>
      {remapNote && (
        <span className="min-w-0 truncate text-text-3" style={{ fontSize: "10.5px" }} title={remapNote} data-testid="review-remap-note" role="status">
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
            showOutdated ? "bg-surface-2 text-text-3 hover:text-text-2" : "bg-surface-2 text-text"
          }`}
          style={{ fontSize: "10.5px" }}
        >
          <History size={11} className="text-st-skipped" /> Show outdated
        </button>
      )}
      {stats && (
        <span className="flex items-center gap-1.5 text-text-2" data-testid="review-stats">
          <span>
            {stats.files_changed} file{stats.files_changed === 1 ? "" : "s"}
          </span>
          <span className="font-mono text-st-done">+{stats.additions}</span>
          <span className="font-mono text-st-failed">−{stats.deletions}</span>
        </span>
      )}
      <span className="h-4 w-px bg-line-strong" />
      <div
        className="inline-flex shrink-0 overflow-hidden rounded border border-line-strong"
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
            effectiveView === "split" ? "bg-surface-2 text-text" : "text-text-3 hover:text-text-2"
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
            effectiveView === "unified" ? "bg-surface-2 text-text" : "text-text-3 hover:text-text-2"
          }`}
          style={{ fontSize: "10.5px" }}
        >
          <Rows3 size={11} /> Unified
        </button>
      </div>
      <button
        type="button"
        onClick={collapseAll}
        disabled={filesCount === 0}
        title="Collapse all / expand all"
        data-testid="review-collapse-all"
        className="flex cursor-pointer items-center gap-1 rounded border border-line-strong bg-surface-2 px-2 py-0.5 text-text-3 hover:text-text-2 disabled:cursor-not-allowed disabled:opacity-50"
        style={{ fontSize: "10.5px" }}
      >
        <ListMinus size={11} /> Collapse all
      </button>
      <ReviewCommentPills
        pairDrafts={pairDrafts}
        send={send}
        sendDisabledReason={sendDisabledReason}
        sendingKeys={sendingKeys}
        sentCount={sentCount}
        unreadComments={unreadComments}
        jumpUnread={jumpUnread}
        jumpComment={jumpComment}
        cycleEntries={cycleEntries}
        pairStates={pairStates}
        toggleShowResolved={toggleShowResolved}
        showResolved={showResolved}
      />
      {/* #30 — `review/toolbar`: the extensions' icon buttons, before « open in a new tab ». */}
      <ExtensionActionButtons
        location="review/toolbar"
        extra={extensionExtra}
        className="flex shrink-0 items-center gap-1"
        buttonClassName="grid h-5 w-[22px] cursor-pointer place-items-center rounded border border-line-strong bg-surface-2 text-text-3 hover:text-text-2"
        iconSize={11}
      />
      <a
        href={reviewUrl(runId, pair, defaults)}
        target="_blank"
        rel="noreferrer"
        title="Open in a new tab — the URL carries the pair"
        data-testid="review-open-new-tab"
        className="grid h-5 w-[22px] place-items-center rounded border border-line-strong bg-surface-2 text-text-3 hover:text-text-2"
      >
        <SquareArrowOutUpRight size={11} />
      </a>
    </div>
  );
}

function goBack() {
  const sameOrigin = document.referrer && document.referrer.startsWith(window.location.origin);
  if (sameOrigin && window.history.length > 1) window.history.back();
  else window.location.assign("/");
}
