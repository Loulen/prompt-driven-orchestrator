import { useEffect, useMemo, useRef } from "react";
import { Bot, Box, Search, User, Wrench } from "lucide-react";
import type { RunTranscriptHit } from "../../../shared/generated/RunTranscriptHit";
import type { TranscriptSnippet } from "../../../shared/generated/TranscriptSnippet";
import { formatTime } from "../../model/nodeDetailFormat";
import { countLabel, groupHits, moveSelection, SEARCH_MAX_HITS } from "../../model/runSearch";
import { useRunSearchStore } from "../../store/runSearchStore";

/** The excerpt of a hit, its match marked. */
function SnippetText({ snippet }: { snippet: TranscriptSnippet }) {
  return (
    <span className="min-w-0 truncate" title={`${snippet.before}${snippet.matched}${snippet.after}`}>
      {snippet.before}
      <mark className="rounded-[3px] bg-st-await px-[2px] text-primary-ink">{snippet.matched}</mark>
      {snippet.after}
    </span>
  );
}

function HitIcon({ hit }: { hit: RunTranscriptHit["hit"] }) {
  const Icon = hit.kind === "tool" ? Wrench : hit.speaker === "prompt" ? User : Bot;
  return <Icon size={11} className="shrink-0 text-text-3" />;
}

/**
 * The search over the Run's transcripts, under the canvas toolbar: the query,
 * « N in M nodes », the hits grouped by node iteration with their excerpt.
 * ↑↓ choose a hit, ↵ opens it in its node's Transcript, Esc closes.
 */
export default function RunSearchPanel({
  nodeName,
  onChoose,
}: {
  nodeName: (nodeId: string) => string;
  onChoose: (hit: RunTranscriptHit) => void;
}) {
  const query = useRunSearchStore((s) => s.query);
  const status = useRunSearchStore((s) => s.status);
  const result = useRunSearchStore((s) => s.result);
  const error = useRunSearchStore((s) => s.error);
  const selected = useRunSearchStore((s) => s.selected);
  const setQuery = useRunSearchStore((s) => s.setQuery);
  const select = useRunSearchStore((s) => s.select);
  const close = useRunSearchStore((s) => s.close);
  const listRef = useRef<HTMLDivElement>(null);
  const hits = useMemo(() => result?.hits ?? [], [result]);
  const groups = useMemo(() => groupHits(hits), [hits]);

  useEffect(() => {
    listRef.current
      ?.querySelector(`[data-hit="${selected}"]`)
      ?.scrollIntoView?.({ block: "nearest" });
  }, [selected]);

  const onKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      select(moveSelection(selected, e.key === "ArrowDown" ? 1 : -1, hits.length));
    } else if (e.key === "Enter") {
      e.preventDefault();
      const hit = hits[selected];
      if (hit) onChoose(hit);
    } else if (e.key === "Escape") {
      e.preventDefault();
      close();
    }
  };

  const typed = query.trim() !== "";
  return (
    <div
      data-testid="run-search-panel"
      className="absolute left-3 top-[50px] z-20 flex max-h-[60%] w-[420px] max-w-[calc(100%-24px)] flex-col overflow-hidden rounded-md border border-line-strong bg-surface-1 shadow-lg"
      style={{ fontSize: "11px" }}
    >
      <div className="flex shrink-0 items-center gap-2 border-b border-line px-2.5 py-1.5">
        <Search size={12} className="shrink-0 text-text-3" />
        <input
          autoFocus
          data-testid="run-search-input"
          aria-label="Search the Run's transcripts"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={onKeyDown}
          placeholder="Search the messages and tool calls of this Run"
          className="min-w-0 flex-1 bg-transparent text-text outline-none placeholder:text-text-3"
          style={{ fontSize: "12px" }}
        />
        {result && typed && (
          <span className="shrink-0 font-mono text-text-3" style={{ fontSize: "10px" }} data-testid="run-search-count">
            {countLabel(result)}
          </span>
        )}
        <kbd
          className="shrink-0 rounded border border-line-strong px-1 font-mono text-text-3"
          style={{ fontSize: "9.5px" }}
        >
          esc
        </kbd>
      </div>
      {typed && (
        <div
          ref={listRef}
          data-testid="run-search-results"
          data-stale={status === "searching" || undefined}
          className={`min-h-0 flex-1 overflow-auto py-1 ${status === "searching" ? "opacity-60" : ""}`}
        >
          {status === "error" && (
            <div className="px-3 py-2 text-st-failed" data-testid="run-search-error">
              {error}
            </div>
          )}
          {status === "searching" && !result && (
            <div className="px-3 py-2 text-text-3" data-testid="run-search-searching">
              Searching…
            </div>
          )}
          {result && result.matched === 0 && status !== "error" && (
            <div className="px-3 py-2 text-text-3" data-testid="run-search-empty">
              No match for « {result.query} » in the transcripts of this Run.
            </div>
          )}
          {groups.map((group) => (
            <div key={`${group.nodeId}:${group.iter}:${group.first}`} data-testid="run-search-group">
              <div className="flex items-center gap-1.5 px-2.5 pb-0.5 pt-1.5 font-mono text-text-3" style={{ fontSize: "10.5px" }}>
                <Box size={11} className="shrink-0 text-text-3" />
                <span data-testid="run-search-group-node">{nodeName(group.nodeId)}</span>
                <span className="text-text-3">iter {group.iter}</span>
              </div>
              {group.hits.map((hit, i) => {
                const at = group.first + i;
                return (
                  <button
                    key={`${hit.hit.kind}:${hit.hit.index}`}
                    type="button"
                    data-testid="run-search-hit"
                    data-hit={at}
                    data-selected={at === selected}
                    onMouseEnter={() => select(at)}
                    onClick={() => onChoose(hit)}
                    className={`mx-1 flex w-[calc(100%-8px)] cursor-pointer items-center gap-1.5 rounded px-1.5 py-1 text-left text-text-2 ${
                      at === selected ? "bg-primary/15" : "hover:bg-hover"
                    }`}
                  >
                    <HitIcon hit={hit.hit} />
                    {hit.hit.kind === "tool" && (
                      <span className="shrink-0 font-mono text-text-3">{hit.hit.name}</span>
                    )}
                    <SnippetText snippet={hit.hit.snippet} />
                    {hit.hit.at && (
                      <span className="ml-auto shrink-0 pl-2 font-mono text-text-3" style={{ fontSize: "9.5px" }}>
                        {formatTime(hit.hit.at)}
                      </span>
                    )}
                  </button>
                );
              })}
            </div>
          ))}
        </div>
      )}
      <div className="shrink-0 border-t border-line px-2.5 py-1 text-text-3" style={{ fontSize: "9.5px" }}>
        ↑↓ choose · ↵ open in the node's Transcript · first {SEARCH_MAX_HITS} hits
        {result?.truncated ? ` of ${result.matched}` : ""}
      </div>
    </div>
  );
}
