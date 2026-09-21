import { useEffect, useMemo, useRef, useState } from "react";
import { Check, ChevronDown, Search } from "lucide-react";
import type { RunRefs } from "../../types";
import { pairOfDelivery, refById, shortSha } from "../../lib/runRefs";
import type { RefPair } from "../../lib/runRefs";

/**
 * One side of the Review page's `source → destination` pair (#749, design
 * option A): a button showing the chosen Run ref, opening a popover with the
 * **Run** group (fork point, Run tip, Working tree while the Run's worktree
 * exists — #835) and the **Deliveries** group — one row per
 * node delivery, whose `before`/`after` (or `● live`) buttons pick that side,
 * and whose header sets **both** sides to the delivery in one click.
 *
 * A ref already used on the other side is disabled: "same ref twice" is an
 * empty state, not a choice worth offering.
 */

interface Props {
  side: "from" | "to";
  value: string;
  /** The ref chosen on the other side (disabled here). */
  other: string;
  refs: RunRefs | null;
  disabled?: boolean;
  onPick: (id: string) => void;
  onPickPair: (pair: RefPair) => void;
}

export default function RefPicker({ side, value, other, refs, disabled, onPick, onPickPair }: Props) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const rootRef = useRef<HTMLDivElement | null>(null);
  const inputRef = useRef<HTMLInputElement | null>(null);

  const current = refs ? refById(refs, value) : undefined;

  useEffect(() => {
    if (!open) return;
    const onDoc = (e: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) {
        setOpen(false);
        setQuery("");
      }
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        setOpen(false);
        setQuery("");
      }
    };
    document.addEventListener("mousedown", onDoc);
    document.addEventListener("keydown", onKey, true);
    const t = window.setTimeout(() => inputRef.current?.focus(), 0);
    return () => {
      document.removeEventListener("mousedown", onDoc);
      document.removeEventListener("keydown", onKey, true);
      window.clearTimeout(t);
    };
  }, [open]);

  const q = query.trim().toLowerCase();
  const matches = (text: string) => !q || text.toLowerCase().includes(q);

  const runRefs = useMemo(
    () => (refs ? refs.refs.filter((r) => r.kind === "fork" || r.kind === "tip" || r.kind === "worktree") : []),
    [refs],
  );

  const close = () => {
    setOpen(false);
    setQuery("");
  };

  const pick = (id: string) => {
    if (id === other) return;
    close();
    onPick(id);
  };

  const isNode = current && current.kind !== "fork" && current.kind !== "tip" && current.kind !== "worktree";
  const isLive = current?.kind === "live";
  const isWorktree = current?.kind === "worktree";

  return (
    <div ref={rootRef} className="relative">
      <button
        type="button"
        disabled={disabled}
        onClick={() => (open ? close() : setOpen(true))}
        aria-haspopup="listbox"
        aria-expanded={open}
        data-testid={`review-${side}`}
        data-ref={value}
        title={current ? `${current.label}${current.sha ? ` · ${current.sha}` : ""}` : value}
        className={`flex max-w-[280px] items-center gap-1.5 rounded border px-2 py-0.5 text-left transition-colors ${
          open ? "border-fg-4 bg-bg-4" : "border-line-strong bg-bg-3 hover:border-fg-4 hover:bg-bg-4"
        } ${disabled ? "cursor-not-allowed opacity-50" : "cursor-pointer"}`}
        style={{ fontSize: "10.5px" }}
      >
        {isNode && (
          <span className="uppercase tracking-wider text-fg-4" style={{ fontSize: "9.5px" }}>
            node
          </span>
        )}
        {(isLive || isWorktree) && (
          <span
            className="inline-block h-1.5 w-1.5 shrink-0 animate-pulse rounded-full bg-st-running"
            aria-hidden
          />
        )}
        <span className="truncate text-fg">{current?.label ?? (refs ? value : "…")}</span>
        {current && (
          <span className="font-mono text-fg-4" style={{ fontSize: "10px" }}>
            {shortSha(current.sha ?? (current.kind === "live" ? current.git_ref : null))}
          </span>
        )}
        <ChevronDown size={10} className="shrink-0 text-fg-4" />
      </button>

      {open && refs && (
        <div
          role="listbox"
          data-testid={`review-ref-popover-${side}`}
          className="absolute left-0 top-[30px] z-50 w-[340px] overflow-hidden rounded-md border border-line-strong bg-bg-2 shadow-[0_12px_32px_rgba(0,0,0,0.55)]"
        >
          <div className="flex items-center gap-1.5 border-b border-line px-2 py-1.5">
            <Search size={11} className="text-fg-4" />
            <input
              ref={inputRef}
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder="Filter refs by node or sha…"
              data-testid={`review-ref-filter-${side}`}
              className="flex-1 rounded border border-line-strong bg-bg-3 px-1.5 py-0.5 text-fg outline-none focus:border-acc-border"
              style={{ fontSize: "11px" }}
            />
          </div>

          <div className="py-1">
            <div
              className="px-2.5 pb-0.5 pt-1 uppercase tracking-wider text-fg-4"
              style={{ fontSize: "9.5px" }}
            >
              Run
            </div>
            {runRefs
              .filter((r) => matches(`${r.label} ${r.sha ?? ""} ${r.git_ref}`))
              .map((r) => {
                const dis = r.id === other;
                const sel = r.id === value;
                return (
                  <button
                    key={r.id}
                    type="button"
                    role="option"
                    aria-selected={sel}
                    disabled={dis}
                    onClick={() => pick(r.id)}
                    data-testid={`review-ref-option-${r.id}`}
                    title={dis ? "Already used on the other side" : r.git_ref}
                    className={`grid w-full grid-cols-[14px_1fr_auto] items-center gap-2 px-2.5 py-1 text-left ${
                      sel ? "bg-acc-bg" : ""
                    } ${dis ? "cursor-not-allowed opacity-45" : "cursor-pointer hover:bg-bg-3"}`}
                    style={{ fontSize: "11px" }}
                  >
                    <span className="text-acc">{sel && <Check size={11} />}</span>
                    <span className="truncate">
                      <span className="text-fg">{r.label}</span>
                      <span className="text-fg-4" style={{ fontSize: "10px" }}>
                        {" "}
                        · {r.git_ref}
                      </span>
                    </span>
                    <span className="font-mono text-fg-4" style={{ fontSize: "10px" }}>
                      {shortSha(r.sha)}
                    </span>
                  </button>
                );
              })}
          </div>

          <div className="border-t border-line py-1">
            <div
              className="flex items-center justify-between px-2.5 pb-0.5 pt-1 uppercase tracking-wider text-fg-4"
              style={{ fontSize: "9.5px" }}
            >
              <span>Deliveries</span>
              <span className="normal-case tracking-normal text-fg-5">click a node to set both sides</span>
            </div>
            {refs.deliveries.length === 0 && (
              <div className="px-2.5 py-1 text-fg-4" style={{ fontSize: "10.5px" }}>
                No node has delivered yet.
              </div>
            )}
            {refs.deliveries
              .filter((d) => matches(`${d.node_name} ${d.node_id} iter ${d.iter}`))
              .map((d) => {
                const before = refById(refs, d.before);
                const after = d.after ? refById(refs, d.after) : undefined;
                const live = d.live ? refById(refs, d.live) : undefined;
                const pair = pairOfDelivery(d);
                const sideBtn = (
                  id: string,
                  label: string,
                  sha: string,
                  liveStyle = false,
                ) => {
                  const dis = id === other;
                  const sel = id === value;
                  return (
                    <button
                      key={id}
                      type="button"
                      role="option"
                      aria-selected={sel}
                      disabled={dis}
                      onClick={() => pick(id)}
                      data-testid={`review-ref-option-${id}`}
                      title={dis ? "Already used on the other side" : id}
                      className={`flex items-center gap-1 rounded border px-1.5 py-px ${
                        sel
                          ? "border-acc-border bg-acc-bg text-acc"
                          : liveStyle
                            ? "border-st-running/40 bg-st-running-bg text-st-running"
                            : "border-line-strong bg-bg-3 text-fg-3 hover:border-fg-4 hover:text-fg"
                      } ${dis ? "cursor-not-allowed opacity-40" : "cursor-pointer"}`}
                      style={{ fontSize: "10px" }}
                    >
                      {liveStyle && (
                        <span className="inline-block h-1.5 w-1.5 animate-pulse rounded-full bg-current" aria-hidden />
                      )}
                      {label}
                      <span className="font-mono text-fg-4">{sha}</span>
                    </button>
                  );
                };
                return (
                  <div key={`${d.node_id}:${d.iter}`}>
                    <button
                      type="button"
                      disabled={!pair}
                      onClick={() => {
                        if (!pair) return;
                        close();
                        onPickPair(pair);
                      }}
                      data-testid={`review-delivery-${d.node_id}-${d.iter}`}
                      title={
                        d.status === "delivered"
                          ? "Review this delivery: before → after"
                          : "Review the live branch: Run tip → live"
                      }
                      className="flex w-full cursor-pointer items-center gap-1.5 px-2.5 pb-px pt-0.5 text-left text-fg-3 hover:bg-bg-3"
                      style={{ fontSize: "10.5px" }}
                    >
                      <span className="text-fg-2">{d.node_name}</span>
                      <span className="font-mono text-fg-4" style={{ fontSize: "9.5px" }}>
                        {d.node_id}
                      </span>
                      <span className="text-fg-4">iter {d.iter}</span>
                      <span
                        className={`rounded-full border px-1.5 ${
                          d.status === "delivered"
                            ? "border-acc-border bg-acc-bg text-acc"
                            : "border-st-running/40 bg-st-running-bg text-st-running"
                        }`}
                        style={{ fontSize: "9.5px" }}
                      >
                        {d.status}
                      </span>
                    </button>
                    <div className="flex gap-1 px-2.5 pb-1 pl-8">
                      {before && sideBtn(d.before, "before", shortSha(before.sha))}
                      {after && d.after && sideBtn(d.after, "after", shortSha(after.sha))}
                      {live && d.live && sideBtn(d.live, "live", live.git_ref, true)}
                    </div>
                  </div>
                );
              })}
          </div>

          <div className="border-t border-line px-2.5 py-1 text-fg-4" style={{ fontSize: "10px" }}>
            Refs are frozen SHAs from the event log; <span className="text-st-running">live</span> follows
            the node's sub-worktree; <span className="text-st-running">Working tree</span> is the Run's worktree
            as it is now, uncommitted edits included.
          </div>
        </div>
      )}
    </div>
  );
}
