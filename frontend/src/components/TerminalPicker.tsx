import { useEffect, useState, type KeyboardEvent, type Ref } from "react";
import { LoaderCircle, Search } from "lucide-react";
import { fetchRun, fetchRuns } from "../api";
import { collectLiveSessions, searchSessions } from "../lib/liveSessions";
import type { TerminalRef } from "../lib/terminalMosaic";

interface Props {
  /** The sessions already in a pane: picking one focuses it instead. */
  shown: string[];
  /** The mosaic is full: only the panes already shown can be picked. */
  full: boolean;
  onPick: (ref: TerminalRef) => void;
  ref?: Ref<HTMLDivElement>;
}

type Listing =
  | { state: "loading" }
  | { state: "error" }
  | { state: "ready"; sessions: TerminalRef[] };

const KIND_LABEL: Record<TerminalRef["kind"], string> = {
  node: "Node",
  manager: "Manager",
  shell: "Shell",
};

/**
 * #1040 — « + terminal »: the live sessions of every Run, searchable by Run and
 * node name. Picking one adds a pane to the Mosaïque — or focuses it, if it is
 * already shown. Read once when opened.
 */
export default function TerminalPicker({ shown, full, onPick, ref }: Props) {
  const [listing, setListing] = useState<Listing>({ state: "loading" });
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);

  useEffect(() => {
    let cancelled = false;
    collectLiveSessions({ fetchRuns, fetchRun })
      .then((sessions) => {
        if (!cancelled) setListing({ state: "ready", sessions });
      })
      .catch(() => {
        if (!cancelled) setListing({ state: "error" });
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const results = listing.state === "ready" ? searchSessions(listing.sessions, query) : [];
  const pickable = (r: TerminalRef) => !full || shown.includes(r.session);
  const activeIndex = Math.min(active, Math.max(0, results.length - 1));

  const onKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    // Typing here is no canvas shortcut.
    e.stopPropagation();
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setActive(Math.min(activeIndex + 1, results.length - 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setActive(Math.max(activeIndex - 1, 0));
    } else if (e.key === "Enter") {
      e.preventDefault();
      const r = results[activeIndex];
      if (r && pickable(r)) onPick(r);
    }
  };

  return (
    <div
      ref={ref}
      className="absolute right-2 top-9 z-20 flex w-[360px] max-w-[calc(100%-1rem)] flex-col overflow-hidden rounded-lg border border-line-strong bg-bg-2 shadow-lg"
      data-testid="terminal-picker"
      role="dialog"
      aria-label="Add a terminal"
    >
      <label className="flex items-center gap-1.5 border-b border-line px-2.5 py-1.5 text-fg-4">
        <Search size={12} aria-hidden />
        <input
          autoFocus
          value={query}
          onChange={(e) => {
            setQuery(e.target.value);
            setActive(0);
          }}
          onKeyDown={onKeyDown}
          placeholder="Search live sessions by Run or node"
          aria-label="Search live sessions by Run or node"
          className="min-w-0 flex-1 bg-transparent text-fg outline-none placeholder:text-fg-4"
          style={{ fontSize: "12px" }}
          data-testid="terminal-picker-search"
        />
      </label>
      {full && (
        <div className="border-b border-line px-2.5 py-1 text-fg-4" style={{ fontSize: "10.5px" }}>
          Four terminals at most: remove one to add another.
        </div>
      )}
      <div className="max-h-[320px] overflow-auto py-1" role="listbox" aria-label="Live sessions">
        {listing.state === "loading" && (
          <div className="flex items-center gap-1.5 px-2.5 py-2 text-fg-4" style={{ fontSize: "11px" }}>
            <LoaderCircle size={12} className="animate-spin" />
            Listing live sessions…
          </div>
        )}
        {listing.state === "error" && (
          <div className="px-2.5 py-2 text-st-failed" style={{ fontSize: "11px" }} data-testid="terminal-picker-error">
            Could not list the live sessions.
          </div>
        )}
        {listing.state === "ready" && results.length === 0 && (
          <div className="px-2.5 py-2 text-fg-4" style={{ fontSize: "11px" }} data-testid="terminal-picker-empty">
            {listing.sessions.length === 0 ? "No live session." : "No live session matches."}
          </div>
        )}
        {results.map((r, i) => {
          const isShown = shown.includes(r.session);
          const enabled = pickable(r);
          return (
            <button
              key={r.session}
              type="button"
              role="option"
              aria-selected={i === activeIndex}
              aria-disabled={enabled ? undefined : true}
              onClick={() => {
                if (enabled) onPick(r);
              }}
              onMouseEnter={() => setActive(i)}
              className={`flex w-full items-center gap-1.5 px-2.5 py-1 text-left transition-colors ${
                i === activeIndex ? "bg-bg-4" : ""
              } ${enabled ? "cursor-pointer" : "cursor-default opacity-50"}`}
              style={{ fontSize: "12px" }}
              data-testid="terminal-picker-option"
              data-session={r.session}
              data-shown={isShown || undefined}
            >
              {r.runName && (
                <>
                  <span className="min-w-0 truncate text-fg-3">{r.runName}</span>
                  <span className="text-fg-4" aria-hidden>
                    ›
                  </span>
                </>
              )}
              <span className="min-w-0 truncate text-fg">{r.subject}</span>
              <span className="flex-1" />
              {isShown && (
                <span className="font-mono text-fg-4" style={{ fontSize: "10px" }}>
                  shown
                </span>
              )}
              <span
                className="rounded border border-line-strong px-1 font-mono text-fg-4"
                style={{ fontSize: "9.5px" }}
              >
                {KIND_LABEL[r.kind]}
              </span>
            </button>
          );
        })}
      </div>
    </div>
  );
}
