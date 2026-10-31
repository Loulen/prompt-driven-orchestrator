import { useEffect, useLayoutEffect, useRef } from "react";
import { Bot, LoaderCircle, MessageSquare, Search, TriangleAlert, Wrench } from "lucide-react";
import { Button, EmptyState } from "../../../../shared/ui/kit";
import type { IterationInfo } from "../../../../shared/generated/IterationInfo";
import { usePinToBottom } from "../../../../hooks/usePinToBottom";
import { openRunSearch } from "../../../model/runSearch";
import { remaining } from "../../../model/transcript";
import { useTranscript } from "../../../store/useTranscript";
import { useSearchHighlight, useTranscriptFocus } from "../../../store/runSearchStore";
import IterSelector from "../IterSelector";
import MessageCard from "./MessageCard";
import ToolCallRow from "./ToolCallRow";
import IterMark from "../../IterMark";

/** How close to the end of the list the next page starts loading, in px. */
const LOAD_AHEAD = 160;

/**
 * The Transcript tab of a node: the iteration's messages and folded tool calls
 * (CONTEXT.md « Transcript »), loaded as the reader scrolls, followed live while
 * the iteration runs — without moving a reader who scrolled up.
 */
export default function TranscriptTab({
  runId,
  nodeId,
  iter,
  iterations,
  onSelectIter,
  live,
}: {
  runId: string;
  nodeId: string;
  iter: number;
  iterations: IterationInfo[];
  onSelectIter: (iter: number) => void;
  live: boolean;
}) {
  const { view, loaded, error, loadMore } = useTranscript(runId, nodeId, iter, live);
  const scrollRef = useRef<HTMLDivElement>(null);
  const { pinnedRef, handleScroll, scrollToBottom } = usePinToBottom(scrollRef, nodeId, iter);
  const more = remaining(view);
  // The Run's search: its word marked, and the hit chosen there brought into
  // view — loading the pages up to it first.
  const highlight = useSearchHighlight(runId);
  const focus = useTranscriptFocus(runId, nodeId, iter);
  const scrolledTo = useRef(0);
  useEffect(() => {
    if (!focus || !loaded) return;
    if (view.entries.length <= focus.index) {
      if (more > 0) loadMore();
      return;
    }
    if (scrolledTo.current === focus.nonce) return;
    scrolledTo.current = focus.nonce;
    scrollRef.current
      ?.querySelector(`[data-entry="${focus.index}"]`)
      ?.scrollIntoView?.({ block: "center" });
    // `view` changes at every read, a live poll's included: a page asked while
    // another read was in flight is asked again.
  }, [focus, loaded, view, more, loadMore]);
  const counts = `${view.messages} ${view.messages === 1 ? "message" : "messages"} · ${view.toolCalls} ${
    view.toolCalls === 1 ? "tool call" : "tool calls"
  }`;

  // The reader at the end of a fully loaded transcript follows what arrives;
  // one reading higher up stays where they are.
  const lastCount = useRef(0);
  useLayoutEffect(() => {
    const grew = view.entries.length > lastCount.current;
    const first = lastCount.current === 0;
    lastCount.current = view.entries.length;
    // The first page lands at the top: measure where the reader is.
    if (first) handleScroll();
    if (!grew || first || more > 0) return;
    if (pinnedRef.current) scrollToBottom();
  }, [view.entries.length, more, pinnedRef, scrollToBottom, handleScroll]);

  // A first page shorter than the pane leaves nothing to scroll: load on.
  useEffect(() => {
    const el = scrollRef.current;
    if (el && more > 0 && el.scrollHeight <= el.clientHeight + LOAD_AHEAD) loadMore();
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

  return (
    <div className="ri-tx" data-testid="transcript-tab">
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
          {view.entries.map((entry) =>
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
              />
            ),
          )}
        </div>
        {more > 0 && (
          <button type="button" data-testid="transcript-more" onClick={loadMore} className="ri-tx__rest ri-tx__more">
            +{more} {more === 1 ? "entry" : "entries"} · loaded as you scroll
          </button>
        )}
      </div>
    </div>
  );
}
