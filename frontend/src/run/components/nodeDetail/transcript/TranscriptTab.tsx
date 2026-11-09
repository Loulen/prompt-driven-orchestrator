import { useEffect, useLayoutEffect, useRef, useState } from "react";
import {
  ArrowDownWideNarrow,
  ArrowUpNarrowWide,
  Bot,
  LoaderCircle,
  MessageSquare,
  Search,
  TriangleAlert,
  Wrench,
} from "lucide-react";
import { Button, EmptyState, Tooltip } from "../../../../shared/ui/kit";
import type { IterationInfo } from "../../../../shared/generated/IterationInfo";
import type { TranscriptOrder } from "../../../../lib/uiPrefs";
import { usePinToEnd } from "../../../../hooks/usePinToBottom";
import { openRunSearch } from "../../../model/runSearch";
import { loadedEnd, newerRemaining, olderRemaining } from "../../../model/transcript";
import { useTranscript } from "../../../store/useTranscript";
import { useTranscriptOrder } from "../../../store/transcriptOrderStore";
import { useSearchHighlight, useTranscriptFocus } from "../../../store/runSearchStore";
import IterSelector from "../IterSelector";
import MessageCard from "./MessageCard";
import ToolCallRow from "./ToolCallRow";
import IterMark from "../../IterMark";

/** How close to the end of the list the next page starts loading, in px. */
const LOAD_AHEAD = 160;

interface TranscriptTabProps {
  runId: string;
  nodeId: string;
  iter: number;
  iterations: IterationInfo[];
  onSelectIter: (iter: number) => void;
  live: boolean;
}

/**
 * The Transcript tab of a node: the iteration's messages and folded tool calls
 * (CONTEXT.md « Transcript »), newest first by default — the order is one
 * choice of this device, switched from the tab's bar. Loaded as the reader
 * scrolls, followed live while the iteration runs — without moving a reader
 * who left the newest end.
 */
export default function TranscriptTab(props: TranscriptTabProps) {
  const order = useTranscriptOrder((s) => s.order);
  // Another order reads the transcript from its other end: start over.
  return <TranscriptList key={order} {...props} order={order} />;
}

const ORDER_TIP: Record<TranscriptOrder, string> = {
  "newest-first": "Newest first · click for oldest first · remembered on this device",
  "oldest-first": "Oldest first · click for newest first · remembered on this device",
};

function TranscriptList({
  runId,
  nodeId,
  iter,
  iterations,
  onSelectIter,
  live,
  order,
}: TranscriptTabProps & { order: TranscriptOrder }) {
  const toggleOrder = useTranscriptOrder((s) => s.toggle);
  const newest = order === "newest-first";
  const { view, loaded, error, loadMore, loadOlder, loadNewer } = useTranscript(runId, nodeId, iter, live, order);
  const scrollRef = useRef<HTMLDivElement>(null);
  const { pinned, pinnedRef, handleScroll, scrollToEnd } = usePinToEnd(scrollRef, nodeId, iter, newest ? "top" : "bottom");
  // What is left to load as the reader scrolls down, away from where the tab opened.
  const more = newest ? olderRemaining(view) : newerRemaining(view);
  const end = loadedEnd(view);
  const first = view.entries[0]?.index ?? end;

  // The Run's search: its word marked, and the hit chosen there brought into
  // view — loading the pages up to it first.
  const highlight = useSearchHighlight(runId);
  const focus = useTranscriptFocus(runId, nodeId, iter);
  const scrolledTo = useRef(0);
  useEffect(() => {
    if (!focus || !loaded) return;
    if (view.entries.length === 0 && view.total > 0) return; // the first page is on its way
    if (focus.index < first) {
      loadOlder();
      return;
    }
    if (focus.index >= end) {
      if (newerRemaining(view) > 0) loadNewer();
      return;
    }
    if (scrolledTo.current === focus.nonce) return;
    scrolledTo.current = focus.nonce;
    // The marked word, when the entry shows one: a tall entry centred whole
    // could leave the word off screen.
    const entry = scrollRef.current?.querySelector(`[data-entry="${focus.index}"]`);
    (entry?.querySelector("mark.query-mark") ?? entry)?.scrollIntoView?.({ block: "center" });
    // `view` changes at every read, a live poll's included: a page asked while
    // another read was in flight is asked again.
  }, [focus, loaded, view, first, end, loadOlder, loadNewer]);
  const counts = `${view.messages} ${view.messages === 1 ? "message" : "messages"} · ${view.toolCalls} ${
    view.toolCalls === 1 ? "tool call" : "tool calls"
  }`;

  // What arrived while the reader was away from the newest end: the « N new »
  // pill. Seen, everything up to the total when the tab opened or when the
  // reader sits at the newest end.
  const [seen, setSeen] = useState<number | null>(null);
  if (loaded && (seen === null || (pinned && seen !== view.total))) setSeen(view.total);
  // Oldest first, entries not loaded yet already say so at the bottom.
  const unseen = !pinned && seen !== null && (newest || more === 0) ? Math.max(0, view.total - seen) : 0;

  // The newest end grew. Oldest first, a reader at the bottom of a fully
  // loaded transcript follows it. Newest first, the new entries land on top: a
  // reader at the top sees them; one reading lower down keeps what they read
  // where it is.
  const lastEnd = useRef(0);
  useLayoutEffect(() => {
    const el = scrollRef.current;
    const prevEnd = lastEnd.current;
    const grew = end > prevEnd;
    const firstPage = prevEnd === 0;
    lastEnd.current = end;
    // The first page lands: measure where the reader is.
    if (firstPage) handleScroll();
    if (!el || !grew || firstPage) return;
    if (newest) {
      if (pinnedRef.current) return;
      // The entry that was on top before: how far the new ones pushed it.
      const anchor = el.querySelector<HTMLElement>(`[data-entry="${prevEnd - 1}"]`);
      const top = el.querySelector<HTMLElement>("[data-entry]");
      // Layout pixels, as `scrollTop` counts them (a rect is scaled by the UI zoom).
      if (anchor && top) el.scrollTop += anchor.offsetTop - top.offsetTop;
    } else if (more === 0 && pinnedRef.current) {
      scrollToEnd();
    }
  }, [end, more, newest, pinnedRef, scrollToEnd, handleScroll]);

  // A first page shorter than the pane leaves nothing to scroll: load on.
  useEffect(() => {
    const el = scrollRef.current;
    if (el && view.entries.length > 0 && more > 0 && el.scrollHeight <= el.clientHeight + LOAD_AHEAD) loadMore();
  }, [view.entries.length, more, loadMore]);

  const onScroll = () => {
    handleScroll();
    const el = scrollRef.current;
    if (el && more > 0 && el.scrollHeight - el.scrollTop - el.clientHeight < LOAD_AHEAD) loadMore();
  };

  if (loaded && !view.available) {
    return (
      <div className="ri-tx" data-testid="transcript-tab">
        <div data-testid="transcript-unavailable">
          <EmptyState icon={view.harness ? MessageSquare : Bot} layout="row">
            {view.harness ? `Unavailable for ${view.harness}` : "No agent on this node"}
          </EmptyState>
        </div>
      </div>
    );
  }

  const entries = newest ? [...view.entries].reverse() : view.entries;
  const backToNewest = () => {
    scrollToEnd();
    setSeen(view.total);
  };

  return (
    <div className="ri-tx" data-testid="transcript-tab" data-order={order}>
      <div className="ri-tx__bar">
        {iterations.length > 1 ? (
          <IterSelector iterations={iterations} selectedIter={iter} onSelect={onSelectIter} />
        ) : (
          <IterMark iter={iter} />
        )}
        <span className="ri-tx__counts" data-testid="transcript-counts" title={`${counts} folded`} aria-label={counts}>
          <MessageSquare size={11} strokeWidth={1.5} aria-hidden />
          <span>{view.messages}</span>
          <Wrench size={11} strokeWidth={1.5} aria-hidden />
          <span>{view.toolCalls}</span>
        </span>
        {live && (
          <span className="ri-tx__live" data-testid="transcript-live" title="The iteration is live">
            <span className="ri-dot" data-live aria-hidden />
            live
          </span>
        )}
        <span className="ri-tx__fill" />
        <Tooltip label={ORDER_TIP[order]}>
          <Button
            tone="ghost"
            size="sm"
            icon={newest ? ArrowDownWideNarrow : ArrowUpNarrowWide}
            data-testid="transcript-order"
            data-order={order}
            aria-label={newest ? "Newest first" : "Oldest first"}
            onClick={toggleOrder}
          />
        </Tooltip>
        <Button
          tone="ghost"
          size="sm"
          icon={Search}
          data-testid="transcript-search-run"
          title="Search the transcripts of the whole Run (⌘⇧F)"
          onClick={() => openRunSearch(runId)}
        >
          Search run
        </Button>
      </div>
      <div className="ri-tx__frame">
        {unseen > 0 && (
          <button
            type="button"
            className="ri-tx__new"
            data-edge={newest ? "top" : "bottom"}
            data-testid="transcript-new"
            onClick={backToNewest}
          >
            {newest ? "↑" : "↓"} {unseen} new
          </button>
        )}
        <div ref={scrollRef} onScroll={onScroll} data-testid="transcript-scroll" className="ri-tx__scroll">
          {error && (
            <div className="ri-tx__err" role="alert" data-testid="transcript-error" title={error}>
              <TriangleAlert size={14} strokeWidth={1.5} aria-hidden />
              <span className="min-w-0 truncate">{error}</span>
            </div>
          )}
          {loaded && !error && view.total === 0 && (
            <div data-testid="transcript-empty">
              <EmptyState icon={live ? LoaderCircle : MessageSquare} layout="row" spin={live}>
                {live ? "Waiting for the first message" : "No message"}
              </EmptyState>
            </div>
          )}
          <div className="ri-tx__list">
            {entries.map((entry) =>
              entry.kind === "message" ? (
                <MessageCard
                  key={entry.index}
                  index={entry.index}
                  speaker={entry.speaker}
                  text={entry.text}
                  at={entry.at}
                  highlight={highlight}
                  focusNonce={focus?.index === entry.index ? focus.nonce : undefined}
                />
              ) : (
                <ToolCallRow
                  key={entry.index}
                  runId={runId}
                  nodeId={nodeId}
                  iter={iter}
                  index={entry.index}
                  name={entry.name}
                  summary={entry.summary}
                  status={entry.status}
                  outputBytes={entry.output_bytes}
                  diff={entry.diff}
                  focusNonce={focus?.index === entry.index ? focus.nonce : undefined}
                  live={live}
                />
              ),
            )}
          </div>
          {more > 0 ? (
            <button type="button" data-testid="transcript-more" onClick={loadMore} className="ri-tx__rest ri-tx__more">
              +{more} {newest ? "older " : ""}
              {more === 1 ? "entry" : "entries"} · loaded as you scroll
            </button>
          ) : (
            newest &&
            view.entries.length > 0 && (
              <div className="ri-tx__rest" data-testid="transcript-start">
                Start of the transcript
              </div>
            )
          )}
        </div>
      </div>
    </div>
  );
}
