import { useEffect, useLayoutEffect, useRef } from "react";
import { ChevronDown, Info, Search } from "lucide-react";
import type { IterationInfo } from "../../../../shared/generated/IterationInfo";
import { usePinToBottom } from "../../../../hooks/usePinToBottom";
import { openRunSearch } from "../../../model/runSearch";
import { remaining } from "../../../model/transcript";
import { useTranscript } from "../../../store/useTranscript";
import { useSearchHighlight, useTranscriptFocus } from "../../../store/runSearchStore";
import IterSelector from "../IterSelector";
import MessageCard from "./MessageCard";
import ToolCallRow from "./ToolCallRow";

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
      <div className="p-3" data-testid="transcript-tab">
        <div
          data-testid="transcript-unavailable"
          className="flex items-start gap-2 rounded-md border border-dashed border-line-strong bg-bg-3 px-3 py-2.5 text-fg-4"
          style={{ fontSize: "11px", lineHeight: 1.5 }}
        >
          <Info size={13} className="mt-px shrink-0" />
          {view.harness
            ? `Transcript unavailable for this harness (${view.harness}). The terminal above still shows the session.`
            : "Transcript unavailable for this node: it runs no agent."}
        </div>
      </div>
    );
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col" data-testid="transcript-tab">
      {/* One line down to a narrow pane: under 400px the shortcut and the
          « folded » word give way, the counts truncate last. */}
      <div
        className="@container flex shrink-0 items-center gap-2 px-3 py-1.5"
        style={{ fontSize: "10.5px" }}
      >
        {iterations.length > 1 ? (
          <span className="shrink-0 rounded border border-line-strong bg-bg-3">
            <IterSelector iterations={iterations} selectedIter={iter} onSelect={onSelectIter} />
          </span>
        ) : (
          <span
            className="shrink-0 whitespace-nowrap rounded border border-line-strong bg-bg-3 px-1.5 py-0.5 font-mono text-fg-3"
            style={{ fontSize: "10px" }}
          >
            iter {iter}
          </span>
        )}
        <span className="min-w-0 truncate text-fg-4" data-testid="transcript-counts" title={counts}>
          {counts}
          <span className="@max-[400px]:hidden"> folded</span>
        </span>
        <span className="ml-auto flex shrink-0 items-center gap-2 whitespace-nowrap">
          {live && (
            <span className="flex items-center gap-1 text-fg-4" data-testid="transcript-live">
              <span className="h-1.5 w-1.5 rounded-full bg-st-running" />
              live
            </span>
          )}
          <button
            type="button"
            data-testid="transcript-search-run"
            onClick={() => openRunSearch(runId)}
            title="Search the transcripts of the whole Run (⌘⇧F)"
            className="flex cursor-pointer items-center gap-1 rounded border border-line-strong px-1.5 py-0.5 text-fg-3 hover:bg-bg-4 hover:text-fg-2"
          >
            <Search size={10} />
            Search run
            <span className="font-mono text-fg-4 @max-[400px]:hidden">⌘⇧F</span>
          </button>
        </span>
      </div>
      <div
        ref={scrollRef}
        onScroll={onScroll}
        data-testid="transcript-scroll"
        className="flex min-h-0 flex-1 flex-col gap-1.5 overflow-auto px-3 pb-3"
      >
        {error && (
          <div className="text-st-failed" style={{ fontSize: "11px" }} data-testid="transcript-error">
            {error}
          </div>
        )}
        {loaded && !error && view.total === 0 && (
          <div className="py-2 text-fg-4" style={{ fontSize: "11px" }} data-testid="transcript-empty">
            {live ? "Waiting for the session's first message…" : "No message in this iteration."}
          </div>
        )}
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
        {more > 0 && (
          <button
            type="button"
            data-testid="transcript-more"
            onClick={loadMore}
            className="flex cursor-pointer items-center gap-1 py-1 text-fg-4 hover:text-fg-2"
            style={{ fontSize: "10.5px" }}
          >
            <ChevronDown size={11} />
            {more} more {more === 1 ? "entry" : "entries"} · loaded as you scroll
          </button>
        )}
      </div>
    </div>
  );
}
