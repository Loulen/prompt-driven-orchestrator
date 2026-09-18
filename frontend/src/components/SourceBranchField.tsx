import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  Check,
  ChevronDown,
  ChevronsRight,
  GitBranch,
  Cloud,
  CloudOff,
  Laptop,
  OctagonX,
  RefreshCw,
  Search,
  TriangleAlert,
} from "lucide-react";
import type { BranchFetchError, BranchRef, FastForwardOutcome } from "../types";
import {
  branchGap,
  canFastForward,
  fastForwardLabel,
  fastForwardTouchesCheckout,
  filterBranches,
  gapLabel,
  groupBranches,
  highlightSegments,
  isBenignRefusal,
  isRetryableRefusal,
  shortAge,
  syncState,
  upstreamSwitchTarget,
  type BranchGap,
  type FastForwardState,
  type SyncState,
} from "../lib/branchSelect";

/**
 * The source-branch control of the launch form (#802, ADR-0070) — a searchable
 * quick pick plus the sync button that carries the selected branch's freshness.
 *
 * ONE component for the primary repo and every secondary row, for the reason
 * #571 already learned the hard way with `pickDefaultBranch`: two copies of a
 * branch control drift, and the drift is invisible until someone launches from
 * the wrong ref.
 *
 * The division of labour on screen is a decision, not an accident (#800
 * grilling): the **field** shows only what was chosen and how old its tip is;
 * the **écart amont is stated in exactly one place**, the sync button. A chip in
 * the field and a chip on the button would be two renderings of one number, free
 * to disagree the moment a refetch lands between renders.
 */

interface Props {
  /** The held `source_branch` / `base_branch`, posted verbatim. */
  value: string;
  onChange: (branch: string) => void;
  branches: BranchRef[];
  /** The list is still loading — the trigger says so and stays inert. */
  loading: boolean;
  /** A fetch is in flight for this repo (spinner, and every state deferred). */
  fetching: boolean;
  /** RFC-3339 date of the last successful fetch, or `null` for "never". */
  lastFetchAt: string | null;
  /** The last fetch's named failure, or `null`. Never blocks a launch. */
  fetchError: BranchFetchError | null;
  /** Refetch all remotes. Called only from the sync popover's explicit button. */
  onFetch: () => void;
  /**
   * Fast-forward a local branch onto its tracking branch (#803, ADR-0070 §2).
   *
   * Called ONLY from the popover's own button — never on open, never on a repo
   * change, never as part of a fetch. Resolves with the outcome (or `null` when a
   * fast-forward was already in flight for this repo and the click was dropped).
   * Optional: a call site with no repo to write to (an edited Trigger's stored
   * branch) simply does not offer the button.
   */
  onFastForward?: (branch: string) => Promise<FastForwardOutcome | null>;
  /** Distinguishes the primary field from each secondary row's. */
  testIdPrefix: string;
  /** `id` of the trigger, so an outer `<label htmlFor>` points at it. */
  id?: string;
  /** Accessible name when there is no visible label (secondary rows). */
  ariaLabel?: string;
}

export default function SourceBranchField({
  value,
  onChange,
  branches,
  loading,
  fetching,
  lastFetchAt,
  fetchError,
  onFetch,
  onFastForward,
  testIdPrefix,
  id,
  ariaLabel,
}: Props) {
  const [pickerOpen, setPickerOpen] = useState(false);
  const [syncOpen, setSyncOpen] = useState(false);
  /**
   * What the LAST fast-forward click produced (#803) — a second layer over the
   * #802 sync state, never a replacement for it. The two answer different
   * questions ("how fresh is this branch?" vs "what happened when I clicked?"),
   * and merging them would make the next fetch erase a refusal someone is still
   * reading.
   *
   * Stored WITH the `owner` it belongs to — one branch, one opening of the popover
   * — so the card retires by itself when either changes. A result about `main` must
   * not be re-read as a result about `develop`, and reopening the popover asks the
   * question again rather than replaying the last answer; the outcome that outlives
   * the card is already in the refreshed list (the ✓ on the sync button).
   */
  const [syncOpenings, setSyncOpenings] = useState(0);
  const [ff, setFf] = useState<{ owner: string; state: FastForwardState }>({
    owner: "",
    state: { kind: "idle" },
  });
  const [query, setQuery] = useState("");
  const [cursor, setCursor] = useState(0);
  const containerRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);

  /**
   * The held value as a branch. When the list does not carry it, that is normally
   * the upstream the sync popover just switched to: #571 drops `origin/main` while
   * a local `main` exists, so the commonest target of the shortcut is precisely the
   * ref that is never listed. Its kind comes from EVIDENCE — some listed branch
   * names it as its upstream — never from the shape of the name, because a *local*
   * branch may legitimately be called `origin/x` (#571).
   */
  const selected = useMemo(() => {
    const listed = branches.find((b) => b.name === value);
    if (listed) return listed;
    if (value && branches.some((b) => b.upstream === value)) {
      return { name: value, kind: "remote" } as BranchRef;
    }
    return undefined;
  }, [branches, value]);

  const { locals, remotes } = useMemo(() => groupBranches(branches), [branches]);
  const visibleLocals = useMemo(() => filterBranches(locals, query), [locals, query]);
  const visibleRemotes = useMemo(() => filterBranches(remotes, query), [remotes, query]);
  // One flat list behind the two rendered groups: ↑↓ walk the rows as they read,
  // crossing the Local → Remote boundary without a special case.
  const navigable = useMemo(
    () => [...visibleLocals, ...visibleRemotes],
    [visibleLocals, visibleRemotes],
  );

  const sync = useMemo(
    () => syncState({ branch: selected, fetching, fetchError, lastFetchAt }),
    [selected, fetching, fetchError, lastFetchAt],
  );

  // Derived, never blanked by an effect: a card whose owner is not the current one
  // simply is not the current card.
  const ffOwner = syncOpen ? `${syncOpenings}:${value}` : "";
  const ffState: FastForwardState =
    ff.owner === ffOwner && ffOwner !== "" ? ff.state : { kind: "idle" };

  /**
   * The fast-forward click (#803, ADR-0070 §2). Optimistic at the click, named at
   * the refusal: what the list knows decides whether the BUTTON shows; what the
   * checkout is at this instant decides whether the move HAPPENS, and only the
   * daemon can answer that without racing.
   */
  const runFastForward = useCallback(async () => {
    if (!onFastForward || !value) return;
    const owner = ffOwner;
    const land = (state: FastForwardState) =>
      // A result that came back after ANOTHER click started must not overwrite it;
      // one that came back after the person moved on is written and then simply
      // never displayed (the derivation above drops it). The refreshed list landed
      // either way — that part is never wasted.
      setFf((prev) => (prev.owner === owner ? { owner, state } : prev));
    setFf({ owner, state: { kind: "running" } });
    const outcome = await onFastForward(value);
    // `null` = a fast-forward was already in flight for this repo and the click was
    // dropped. Nothing was asked, so nothing is reported: dropping back to idle lets
    // the in-flight one speak for itself.
    if (!outcome) return land({ kind: "idle" });
    if (outcome.kind === "done") return land({ kind: "done", result: outcome.result });
    if (outcome.kind === "error") return land({ kind: "error", message: outcome.message });
    // The two benign races (`up_to_date`, `no_upstream`) came back with a refreshed
    // list that already tells the truth. Going back to idle shows the matching #802
    // state instead of an error card — nothing went wrong, we were a beat late.
    land(
      isBenignRefusal(outcome.refusal)
        ? { kind: "idle" }
        : { kind: "refused", refusal: outcome.refusal },
    );
  }, [onFastForward, value, ffOwner]);

  const closePicker = useCallback(() => {
    setPickerOpen(false);
    setQuery("");
    triggerRef.current?.focus();
  }, []);

  const openPicker = useCallback(() => {
    setQuery("");
    // Open ON the current selection, so ↓ walks away from where you are rather
    // than from the top of a list you did not choose.
    const at = navigable.findIndex((b) => b.name === value);
    setCursor(at === -1 ? 0 : at);
    setSyncOpen(false);
    setPickerOpen(true);
  }, [navigable, value]);

  const pick = useCallback(
    (name: string) => {
      onChange(name);
      closePicker();
    },
    [onChange, closePicker],
  );

  // One popover open at a time, and neither survives a click elsewhere, an Escape,
  // or the window losing focus. All three are handled by the same effect so the two
  // can never end up stacked or stranded — including when Settings opens OVER the
  // still-mounted dialog (#691), whose first click lands outside this container.
  useEffect(() => {
    if (!pickerOpen && !syncOpen) return;
    const closeAll = () => {
      setPickerOpen(false);
      setSyncOpen(false);
      setQuery("");
    };
    const onPointerDown = (e: MouseEvent) => {
      if (!containerRef.current?.contains(e.target as Node)) closeAll();
    };
    const onKeyDown = (e: KeyboardEvent) => {
      // Stopped here so Escape closes the popover instead of the whole dialog
      // underneath it — losing a half-filled form to a dismissed popover would be
      // a far worse surprise than one extra keystroke.
      if (e.key !== "Escape") return;
      e.stopPropagation();
      closeAll();
      triggerRef.current?.focus();
    };
    document.addEventListener("mousedown", onPointerDown);
    document.addEventListener("keydown", onKeyDown, true);
    window.addEventListener("blur", closeAll);
    return () => {
      document.removeEventListener("mousedown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown, true);
      window.removeEventListener("blur", closeAll);
    };
  }, [pickerOpen, syncOpen]);

  // The cursor must stay inside a list the filter just shortened, or ⏎ would
  // select nothing (or, worse, whatever slid into the old index).
  const cursorInRange = Math.min(cursor, Math.max(0, navigable.length - 1));

  const onSearchKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setCursor((c) => Math.min(navigable.length - 1, Math.min(c, navigable.length - 1) + 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setCursor((c) => Math.max(0, Math.min(c, navigable.length - 1) - 1));
    } else if (e.key === "Enter") {
      e.preventDefault();
      const target = navigable[cursorInRange];
      if (target) pick(target.name);
    }
    // Escape is NOT handled here: the effect above catches it on document capture
    // (so it works for the sync popover too) and stops it before it reaches this
    // input. One handler, one behaviour — abandon, and put focus back.
  };

  const selectedAge = shortAge(selected?.last_commit_at);
  const triggerDisabled = loading && branches.length === 0;

  return (
    <div ref={containerRef} className="relative flex flex-col gap-1.5">
      <div className="flex items-start gap-1.5">
        {/* ── The field: what was chosen, and how old its tip is. No écart. ── */}
        <div className="relative flex-1 min-w-0">
          <button
            ref={triggerRef}
            id={id}
            type="button"
            aria-label={ariaLabel}
            aria-haspopup="listbox"
            aria-expanded={pickerOpen}
            disabled={triggerDisabled}
            onClick={() => (pickerOpen ? closePicker() : openPicker())}
            onKeyDown={(e) => {
              if (!pickerOpen && (e.key === "ArrowDown" || e.key === "ArrowUp")) {
                e.preventDefault();
                openPicker();
              }
            }}
            className={`flex w-full items-center gap-2 rounded-md border bg-bg-3 px-2.5 py-1.5 text-left font-mono text-fg transition-colors focus:outline-none disabled:opacity-40 ${
              pickerOpen ? "border-acc" : "border-line-strong hover:border-fg-4"
            }`}
            style={{ fontSize: "12px" }}
            data-testid={`${testIdPrefix}-trigger`}
          >
            <KindIcon kind={selected?.kind} />
            <span className="min-w-0 flex-1 truncate">
              {value || (triggerDisabled ? "Loading branches…" : "Select a branch")}
            </span>
            {selectedAge && (
              <span className="shrink-0 text-fg-4" style={{ fontSize: "11px" }}>
                {selectedAge}
              </span>
            )}
            <ChevronDown size={12} className="shrink-0 text-fg-4" />
          </button>

          {pickerOpen && (
            <div
              className="absolute left-0 right-0 top-full z-50 mt-1 overflow-hidden rounded-md border border-line-strong bg-bg-4 shadow-xl"
              data-testid={`${testIdPrefix}-popover`}
            >
              <div className="flex items-center gap-2 border-b border-line px-2.5 py-2">
                <Search size={12} className="shrink-0 text-fg-4" />
                <input
                  autoFocus
                  value={query}
                  onChange={(e) => {
                    setQuery(e.target.value);
                    setCursor(0);
                  }}
                  onKeyDown={onSearchKeyDown}
                  placeholder="Filter branches…"
                  className="min-w-0 flex-1 bg-transparent text-fg placeholder:text-fg-4 focus:outline-none"
                  style={{ fontSize: "12px" }}
                  data-testid={`${testIdPrefix}-search`}
                  aria-label="Filter branches"
                />
                <span className="shrink-0 text-fg-4" style={{ fontSize: "10px" }}>
                  ↑↓ · ⏎ · esc
                </span>
              </div>

              <div className="max-h-64 overflow-y-auto" role="listbox">
                {navigable.length === 0 && (
                  <div
                    className="px-2.5 py-3 text-fg-4"
                    style={{ fontSize: "11px" }}
                    data-testid={`${testIdPrefix}-empty`}
                  >
                    No branch matches “{query}”.
                  </div>
                )}
                <BranchGroup
                  label="Local"
                  kind="local"
                  rows={visibleLocals}
                  offset={0}
                  cursor={cursorInRange}
                  value={value}
                  query={query}
                  onPick={pick}
                  onHover={setCursor}
                  testIdPrefix={testIdPrefix}
                />
                <BranchGroup
                  label="Remote"
                  kind="remote"
                  rows={visibleRemotes}
                  offset={visibleLocals.length}
                  cursor={cursorInRange}
                  value={value}
                  query={query}
                  onPick={pick}
                  onHover={setCursor}
                  testIdPrefix={testIdPrefix}
                />
              </div>

              <div
                className="border-t border-line px-2.5 py-1.5 text-fg-4"
                style={{ fontSize: "10px" }}
                data-testid={`${testIdPrefix}-footer`}
              >
                {locals.length} local · {remotes.length} remote · {fetchedLabel(lastFetchAt, fetchError, fetching)}
              </div>
            </div>
          )}
        </div>

        {/* ── The sync button: the ONE place the selection's écart is shown. ── */}
        <SyncButton
          state={sync}
          ff={ffState}
          open={syncOpen}
          onToggle={() => {
            // A click opens the state; it never refetches by itself — the fetch is
            // an action you choose in the popover, having read what it will do.
            setPickerOpen(false);
            setSyncOpen((o) => !o);
            // A new opening retires the previous card (see `ff` above).
            setSyncOpenings((n) => n + 1);
          }}
          onFetch={onFetch}
          // #803: only ever offered when the LIST says the branch is a local one
          // strictly behind its upstream. The other conditions are click-time.
          onFastForward={
            onFastForward && canFastForward(selected) ? runFastForward : undefined
          }
          touchesCheckout={fastForwardTouchesCheckout(selected)}
          onSwitchToUpstream={() => {
            const target = upstreamSwitchTarget(selected);
            if (target) {
              onChange(target);
              setSyncOpen(false);
            }
          }}
          upstream={upstreamSwitchTarget(selected)}
          branchName={value}
          testIdPrefix={testIdPrefix}
        />
      </div>
    </div>
  );
}

/**
 * Laptop for a local branch, cloud for a remote-tracking ref — and the neutral
 * branch glyph when the kind is genuinely unknown (a value the repo no longer
 * lists, e.g. an edited Trigger's stored branch). Guessing from the name is never
 * an option: a *local* branch may be called `origin/x` (#571).
 */
function KindIcon({ kind, size = 12 }: { kind?: "local" | "remote"; size?: number }) {
  const className = "shrink-0 text-fg-3";
  if (kind === "remote") return <Cloud size={size} className={className} aria-hidden />;
  if (kind === "local") return <Laptop size={size} className={className} aria-hidden />;
  return <GitBranch size={size} className={className} aria-hidden />;
}

/** "fetched 12 s ago" · "fetched — failed" · "never fetched". */
function fetchedLabel(
  lastFetchAt: string | null,
  fetchError: BranchFetchError | null,
  fetching: boolean,
): string {
  if (fetching) return "fetching…";
  if (fetchError) return "fetched — failed";
  const age = shortAge(lastFetchAt);
  return age ? `fetched ${age} ago` : "never fetched";
}

function BranchGroup({
  label,
  kind,
  rows,
  offset,
  cursor,
  value,
  query,
  onPick,
  onHover,
  testIdPrefix,
}: {
  label: string;
  kind: "local" | "remote";
  rows: BranchRef[];
  /** Index of this group's first row in the flat navigable list. */
  offset: number;
  cursor: number;
  value: string;
  query: string;
  onPick: (name: string) => void;
  onHover: (index: number) => void;
  testIdPrefix: string;
}) {
  if (rows.length === 0) return null;
  return (
    <>
      <div
        className="flex items-center gap-1.5 px-2.5 pt-2 pb-1 uppercase tracking-wider text-fg-4"
        style={{ fontSize: "9.5px" }}
      >
        <KindIcon kind={kind} size={10} />
        {label}
        <span className="text-fg-5">{rows.length}</span>
      </div>
      {rows.map((branch, i) => {
        const index = offset + i;
        const isSelected = branch.name === value;
        const isCursor = index === cursor;
        return (
          <button
            key={`${kind}-${branch.name}`}
            type="button"
            role="option"
            aria-selected={isSelected}
            onMouseDown={(e) => {
              e.preventDefault();
              onPick(branch.name);
            }}
            onMouseEnter={() => onHover(index)}
            className={`flex w-full items-center gap-2 px-2.5 py-1.5 text-left transition-colors ${
              isSelected ? "bg-acc-bg" : isCursor ? "bg-bg-5" : ""
            }`}
            data-testid={`${testIdPrefix}-option`}
            data-branch={branch.name}
            data-cursor={isCursor ? "true" : undefined}
          >
            <KindIcon kind={branch.kind} />
            <span className="flex min-w-0 flex-1 flex-col leading-tight">
              <span
                className={`truncate font-mono ${isSelected ? "text-acc" : "text-fg"}`}
                style={{ fontSize: "11.5px" }}
              >
                {highlightSegments(branch.name, query).map((seg, s) => (
                  <span key={s} className={seg.match ? "text-acc" : undefined}>
                    {seg.text}
                  </span>
                ))}
              </span>
              {branch.last_commit_subject && (
                <span className="truncate text-fg-4" style={{ fontSize: "10px" }}>
                  {branch.last_commit_subject}
                </span>
              )}
            </span>
            <GapChip gap={branchGap(branch)} />
            {shortAge(branch.last_commit_at) && (
              <span className="shrink-0 text-fg-4" style={{ fontSize: "10px" }}>
                {shortAge(branch.last_commit_at)}
              </span>
            )}
          </button>
        );
      })}
    </>
  );
}

/**
 * The per-row écart. Nothing at all for a remote ref or a branch that tracks no
 * upstream — the absence IS the statement (there is no comparison to make), and
 * a `✓` there would claim an agreement that does not exist.
 */
function GapChip({ gap }: { gap: BranchGap | null }) {
  if (!gap) return null;
  if (gap.kind === "level") {
    return (
      <Check size={11} className="shrink-0 text-fg-4" aria-label="up to date" />
    );
  }
  if (gap.kind === "unknown") {
    return (
      <span
        className="shrink-0 font-mono text-fg-4"
        style={{ fontSize: "10px" }}
        title="Gap unknown — the last fetch failed"
      >
        ?
      </span>
    );
  }
  const ahead = gap.kind === "ahead";
  return (
    <span
      className={`shrink-0 rounded px-1 py-0.5 font-mono font-medium ${
        ahead ? "bg-st-running-bg text-st-running" : "bg-st-await-bg text-st-await"
      }`}
      style={{ fontSize: "9.5px" }}
      data-testid="branch-gap-chip"
    >
      {gapLabel(gap)}
    </span>
  );
}

function SyncButton({
  state,
  ff,
  open,
  onToggle,
  onFetch,
  onFastForward,
  touchesCheckout,
  onSwitchToUpstream,
  upstream,
  branchName,
  testIdPrefix,
}: {
  state: SyncState;
  ff: FastForwardState;
  open: boolean;
  onToggle: () => void;
  onFetch: () => void;
  /** `undefined` = no fast-forward is on offer for this selection. */
  onFastForward?: () => void;
  touchesCheckout?: boolean;
  onSwitchToUpstream: () => void;
  upstream: string | null;
  branchName: string;
  testIdPrefix: string;
}) {
  const gap = state.kind === "gap" ? state.gap : null;
  let border = "border-line-strong";
  let icon = <RefreshCw size={12} className="text-fg-3" />;
  let title = "Source branch freshness";

  switch (state.kind) {
    case "fetching":
      icon = <RefreshCw size={12} className="animate-spin text-fg-4" />;
      title = "Fetching all remotes…";
      break;
    case "unknown":
      border = "border-dashed border-fg-4";
      icon = <CloudOff size={12} className="text-fg-3" />;
      title = "Gap unknown — the last fetch failed";
      break;
    case "remote":
      border = "border-acc-border";
      icon = <Cloud size={12} className="text-acc" />;
      title = "Remote ref — as of the last fetch";
      break;
    case "no-upstream":
      icon = <Laptop size={12} className="text-fg-3" />;
      title = "No upstream branch";
      break;
    case "level":
      border = "border-acc-border";
      icon = <Check size={12} className="text-acc" />;
      title = `Up to date with ${state.upstream}`;
      break;
    case "gap":
      border = "border-st-await";
      icon = <RefreshCw size={12} className="text-st-await" />;
      title = `${gapLabel(state.gap)} vs ${state.upstream}`;
      break;
  }

  // #803: while the fast-forward is out, the trigger says so rather than keep
  // showing the écart it is in the middle of erasing. There is deliberately NO
  // `done` case: the refreshed list has already turned the state to `level`, so
  // the ✓ is the same ✓ any up-to-date branch gets — one source of truth for
  // "this branch is level", not a badge we paint ourselves after a click.
  const running = ff.kind === "running";
  if (running) {
    border = "border-acc-border";
    icon = <RefreshCw size={12} className="animate-spin text-acc" />;
    title = `Fast-forwarding ${branchName}…`;
  }

  return (
    <div className="relative shrink-0">
      <button
        type="button"
        onClick={onToggle}
        title={title}
        aria-label={title}
        aria-expanded={open}
        className={`flex items-center gap-1 rounded-md border bg-bg-3 px-2 py-1.5 transition-colors hover:bg-bg-4 ${border}`}
        data-testid={`${testIdPrefix}-sync`}
        data-sync-state={running ? "fast-forwarding" : state.kind}
      >
        {icon}
        {/* The ONLY écart on the form. Icon-only whenever there is none, so the
            button stays quiet until it has something to say. */}
        {gap && !running && (
          <span
            className="font-mono font-medium text-st-await"
            style={{ fontSize: "10px" }}
            data-testid={`${testIdPrefix}-sync-gap`}
          >
            {gapLabel(gap)}
          </span>
        )}
      </button>

      {open && (
        <SyncPopover
          state={state}
          ff={ff}
          onFetch={onFetch}
          onFastForward={onFastForward}
          touchesCheckout={touchesCheckout}
          onSwitchToUpstream={onSwitchToUpstream}
          upstream={upstream}
          branchName={branchName}
          testIdPrefix={testIdPrefix}
        />
      )}
    </div>
  );
}

function SyncPopover({
  state,
  ff,
  onFetch,
  onFastForward,
  touchesCheckout,
  onSwitchToUpstream,
  upstream,
  branchName,
  testIdPrefix,
}: {
  state: SyncState;
  ff: FastForwardState;
  onFetch: () => void;
  onFastForward?: () => void;
  touchesCheckout?: boolean;
  onSwitchToUpstream: () => void;
  upstream: string | null;
  branchName: string;
  testIdPrefix: string;
}) {
  // #803: a fast-forward in flight defers every button — including "Fetch again",
  // whose refreshed list would land on top of a move still being decided.
  const frozen = ff.kind === "running";

  const fetchAgain = (
    <button
      type="button"
      onClick={onFetch}
      disabled={state.kind === "fetching" || frozen}
      className="flex items-center gap-1.5 rounded border border-line-strong bg-bg-3 px-2 py-1 font-medium text-fg-2 transition-colors hover:bg-bg-5 disabled:opacity-40"
      style={{ fontSize: "10.5px" }}
      data-testid={`${testIdPrefix}-sync-fetch`}
    >
      <RefreshCw size={11} className={state.kind === "fetching" ? "animate-spin" : undefined} />
      Fetch again
    </button>
  );

  const launchFromUpstream = upstream ? (
    <button
      type="button"
      onClick={onSwitchToUpstream}
      disabled={frozen}
      className="flex items-center gap-1.5 rounded border border-acc-border bg-acc-bg px-2 py-1 font-medium text-acc transition-colors hover:bg-acc/20 disabled:opacity-40"
      style={{ fontSize: "10.5px" }}
      data-testid={`${testIdPrefix}-sync-switch`}
    >
      <Cloud size={11} />
      Launch from {upstream}
    </button>
  ) : null;

  /** The same shortcut, stepped DOWN to neutral: it is the fallback, not the offer. */
  const launchFromUpstreamSecondary = upstream ? (
    <button
      type="button"
      onClick={onSwitchToUpstream}
      disabled={frozen}
      className="flex items-center gap-1.5 rounded border border-line-strong bg-bg-3 px-2 py-1 font-medium text-fg-2 transition-colors hover:bg-bg-5 disabled:opacity-40"
      style={{ fontSize: "10.5px" }}
      data-testid={`${testIdPrefix}-sync-switch`}
    >
      <Cloud size={11} />
      Launch from {upstream}
    </button>
  ) : null;

  const fastForwardButton = onFastForward ? (
    <button
      type="button"
      onClick={onFastForward}
      disabled={frozen}
      className="flex items-center gap-1.5 rounded border border-acc-border bg-acc-bg px-2 py-1 font-medium text-acc transition-colors hover:bg-acc/20 disabled:opacity-40"
      style={{ fontSize: "10.5px" }}
      data-testid={`${testIdPrefix}-sync-ff`}
    >
      <ChevronsRight size={11} />
      {frozen ? "Fast-forwarding…" : fastForwardLabel(branchName)}
    </button>
  ) : null;

  return (
    <div
      className="absolute right-0 top-full z-50 mt-1 w-[300px] rounded-md border border-line-strong bg-bg-4 p-3 shadow-xl"
      data-testid={`${testIdPrefix}-sync-popover`}
      data-sync-state={state.kind}
      data-ff-state={ff.kind}
    >
      {/* ── The fast-forward leg (#803), when the last click produced something.
          It replaces the freshness card rather than stacking under it: the person
          clicked to change the answer, so the answer is what the card must be. ── */}
      {ff.kind !== "idle" ? (
        <FastForwardCard
          ff={ff}
          branchName={branchName}
          upstream={upstream}
          fetchAgain={fetchAgain}
          fastForwardButton={fastForwardButton}
          launchFromUpstream={launchFromUpstream}
          launchFromUpstreamSecondary={launchFromUpstreamSecondary}
          onRetry={onFastForward}
          testIdPrefix={testIdPrefix}
        />
      ) : (
        <>
      {state.kind === "fetching" && (
        <>
          <Title icon={<RefreshCw size={12} className="animate-spin text-fg-3" />}>
            Fetching all remotes…
          </Title>
          <div className="mt-2 flex flex-wrap gap-1.5">{fetchAgain}</div>
        </>
      )}

      {state.kind === "gap" && (
        <>
          <Title
            icon={
              state.gap.kind === "diverged" ? (
                <GitBranch size={12} className="text-st-await" />
              ) : (
                <TriangleAlert size={12} className="text-st-await" />
              )
            }
          >
            {gapSentence(branchName, state.gap, state.upstream)}
          </Title>
          <Body>
            {/* #803 replaces #802's "PDO never pulls into your checkout" with a
                sentence that says what the fast-forward DOES and does not do —
                the promise is only worth making where the button exists. */}
            {fastForwardButton ? (
              <>
                {fetchedSentence(state.lastFetchAt)} Fast-forward moves{" "}
                <span className="font-mono text-fg-2">{branchName}</span> onto{" "}
                <span className="font-mono text-fg-2">{state.upstream}</span> — no merge,
                no rebase, no push.{" "}
                {touchesCheckout === false
                  ? `${branchName} isn't checked out: only the ref moves, none of your files change.`
                  : "Or launch from the upstream ref and leave it alone."}
              </>
            ) : state.gap.kind === "diverged" ? (
              <>
                A fast-forward would drop your local commit
                {state.gap.ahead > 1 ? "s" : ""}, so none is offered. Launch from the
                upstream ref, or reconcile in your terminal and fetch again.
              </>
            ) : (
              <>
                {fetchedSentence(state.lastFetchAt)} PDO never pulls into your checkout:
                launch from the upstream ref to start from the fresh commits, or keep{" "}
                <span className="font-mono text-fg-2">{branchName}</span> as is.
              </>
            )}
          </Body>
          {/* Behind and lossless: the fast-forward leads — it repairs the thing the
              person already chose, which is the whole reason #803 exists. The #802
              shortcut steps down to neutral, still there, no longer the offer. */}
          <div className="mt-2 flex flex-wrap gap-1.5">
            {fastForwardButton ?? launchFromUpstream}
            {fastForwardButton ? launchFromUpstreamSecondary : null}
            {fetchAgain}
          </div>
        </>
      )}

      {state.kind === "level" && (
        <>
          <Title icon={<Check size={12} className="text-acc" />}>
            Up to date with <span className="font-mono">{state.upstream}</span>
          </Title>
          <Body>{fetchedSentence(state.lastFetchAt)}</Body>
          <div className="mt-2 flex flex-wrap gap-1.5">{fetchAgain}</div>
        </>
      )}

      {state.kind === "unknown" && (
        <>
          <Title icon={<CloudOff size={12} className="text-fg-3" />}>
            Gap unknown — fetch failed
          </Title>
          <Body>
            {state.lastFetchAt
              ? `Last successful fetch ${shortAge(state.lastFetchAt)} ago.`
              : "No successful fetch on record."}{" "}
            Launching is still possible: the run will cut from{" "}
            <span className="font-mono text-fg-2">{branchName}</span> as it is on disk.
          </Body>
          <div
            className="mt-2 max-h-16 overflow-y-auto rounded border border-line bg-bg-2 px-1.5 py-1 font-mono text-fg-3"
            style={{ fontSize: "9.5px" }}
            data-testid={`${testIdPrefix}-sync-stderr`}
          >
            {state.error.message}
          </div>
          {/* Fetch leads here: the failure is what the user wants to retry, and
              the shortcut would launch from a ref nobody could refresh. */}
          <div className="mt-2 flex flex-wrap gap-1.5">
            {fetchAgain}
            {launchFromUpstream}
          </div>
        </>
      )}

      {state.kind === "remote" && (
        <>
          <Title icon={<Cloud size={12} className="text-acc" />}>
            Remote ref — as of last fetch
          </Title>
          <Body>{fetchedSentence(state.lastFetchAt)}</Body>
          <div className="mt-2 flex flex-wrap gap-1.5">{fetchAgain}</div>
        </>
      )}

      {state.kind === "no-upstream" && (
        <>
          <Title icon={<Laptop size={12} className="text-fg-3" />}>No upstream branch</Title>
          <Body>
            <span className="font-mono text-fg-2">{branchName || "This branch"}</span> tracks
            nothing, so there is no gap to measure. {fetchedSentence(state.lastFetchAt)}
          </Body>
          <div className="mt-2 flex flex-wrap gap-1.5">{fetchAgain}</div>
        </>
      )}
        </>
      )}

      {/* Two standing promises, in the one place every state shows: WHEN PDO
          reaches the network, and what it will never write. `flex-wrap` because
          the second is long enough to need its own line at this width — and
          truncating a promise is worse than wrapping it. */}
      <div
        className="mt-2 flex flex-wrap items-center justify-between gap-x-2 border-t border-line pt-1.5 text-fg-5"
        style={{ fontSize: "9px" }}
      >
        <span>fetch on open · on repo change · here</span>
        <span>never merges, rebases or pushes</span>
      </div>
    </div>
  );
}

/**
 * What the fast-forward click produced (#803) — in flight, done, or refused.
 *
 * The two refusals that reach here are the two that could NOT be known before the
 * click (a dirty tree, a branch held by another worktree). Both are named with the
 * daemon's own reason code, because the code is what the person will search for and
 * what a bug report should carry; and both lead with the shortcut, which is the exit
 * that works regardless of the state of any checkout.
 */
function FastForwardCard({
  ff,
  branchName,
  upstream,
  fetchAgain,
  fastForwardButton,
  launchFromUpstream,
  launchFromUpstreamSecondary,
  onRetry,
  testIdPrefix,
}: {
  ff: FastForwardState;
  branchName: string;
  upstream: string | null;
  fetchAgain: React.ReactNode;
  fastForwardButton: React.ReactNode;
  launchFromUpstream: React.ReactNode;
  launchFromUpstreamSecondary: React.ReactNode;
  onRetry?: () => void;
  testIdPrefix: string;
}) {
  if (ff.kind === "running") {
    return (
      <>
        <Title icon={<RefreshCw size={12} className="animate-spin text-acc" />}>
          Fast-forwarding <span className="font-mono">{branchName}</span>…
        </Title>
        <Body>Checking the checkout is clean, then moving the ref.</Body>
        {/* The buttons stay in place, deferred rather than removed: a layout that
            reshuffles under the cursor for one second is its own small betrayal.
            Closing now is safe and nothing here says otherwise — the POST is
            already out, and it finishes server-side either way. */}
        <div className="mt-2 flex flex-wrap gap-1.5">
          {fastForwardButton}
          {launchFromUpstreamSecondary}
        </div>
      </>
    );
  }

  if (ff.kind === "done") {
    const { result } = ff;
    return (
      <>
        <Title icon={<Check size={12} className="text-acc" />}>
          <span className="font-mono">{result.branch}</span> fast-forwarded to{" "}
          <span className="font-mono">{result.upstream}</span>
        </Title>
        <Body>
          {result.commits} commit{result.commits > 1 ? "s" : ""}.{" "}
          {result.moved_checkout
            ? `Your checkout at ${shortPath(result.moved_checkout)} now sits on the new tip.`
            : "No file changed: the branch was checked out nowhere, so only the ref moved."}
        </Body>
        <div
          className="mt-2 rounded border border-line bg-bg-2 px-1.5 py-1 font-mono text-fg-3"
          style={{ fontSize: "9.5px" }}
          data-testid={`${testIdPrefix}-sync-ff-moved`}
        >
          {result.from} → {result.to}
          {result.subject ? ` · ${result.subject}` : ""}
        </div>
        <div className="mt-2 flex flex-wrap gap-1.5">{fetchAgain}</div>
      </>
    );
  }

  if (ff.kind === "error") {
    return (
      <>
        <Title icon={<CloudOff size={12} className="text-fg-3" />}>
          The fast-forward could not be sent
        </Title>
        <Body>
          Nothing moved: <span className="font-mono text-fg-2">{branchName}</span> is
          exactly as it was. Launching is unaffected.
        </Body>
        <div
          className="mt-2 max-h-16 overflow-y-auto rounded border border-line bg-bg-2 px-1.5 py-1 font-mono text-fg-3"
          style={{ fontSize: "9.5px" }}
          data-testid={`${testIdPrefix}-sync-ff-error`}
        >
          {ff.message}
        </div>
        <div className="mt-2 flex flex-wrap gap-1.5">
          {launchFromUpstream}
          {fetchAgain}
        </div>
      </>
    );
  }

  if (ff.kind !== "refused") return null;
  const { refusal } = ff;
  const dirty = refusal.reason === "dirty_tree";
  return (
    <>
      <Title icon={<OctagonX size={12} className="text-st-failed" />}>
        <span className="flex flex-wrap items-baseline gap-1.5">
          <span>
            Can&apos;t fast-forward —{" "}
            {dirty ? "working tree not clean" : "checked out in another worktree"}
          </span>
          <span
            className="rounded bg-bg-2 px-1 py-0.5 font-mono font-normal text-fg-4"
            style={{ fontSize: "9px" }}
            data-testid={`${testIdPrefix}-sync-ff-reason`}
          >
            {refusal.reason}
          </span>
        </span>
      </Title>
      <Body>
        <span className="font-mono text-fg-2">{branchName}</span> is checked out at{" "}
        <span className="font-mono text-fg-2">
          {refusal.checkout ? shortPath(refusal.checkout) : "another checkout"}
        </span>
        {dirty ? (
          <>
            {" "}
            and {refusal.dirty_count ?? refusal.dirty_files?.length ?? 0} tracked file
            {(refusal.dirty_count ?? 1) > 1 ? "s are" : " is"} modified. PDO won&apos;t
            touch them. Commit or stash, then try again
            {upstream ? <> — or launch from {upstream}.</> : "."}
          </>
        ) : (
          <>
            . PDO only fast-forwards a branch in the repository you launch from.
            {upstream ? <> Launch from {upstream} instead.</> : ""}
          </>
        )}
      </Body>
      {dirty && (refusal.dirty_files?.length ?? 0) > 0 && (
        <div
          className="mt-2 max-h-16 overflow-y-auto rounded border border-line bg-bg-2 px-1.5 py-1 font-mono text-fg-3"
          style={{ fontSize: "9.5px" }}
          data-testid={`${testIdPrefix}-sync-ff-dirty`}
        >
          {refusal.dirty_files?.map((line: string) => (
            <div key={line} className="truncate">
              {line}
            </div>
          ))}
        </div>
      )}
      {/* The shortcut leads: it is the exit that works no matter what any checkout
          is doing. "Try again" only where the person can act on the cause — retrying
          a branch someone else holds is an invitation to a loop. */}
      <div className="mt-2 flex flex-wrap gap-1.5">
        {launchFromUpstream}
        {onRetry && isRetryableRefusal(refusal) && (
          <button
            type="button"
            onClick={onRetry}
            className="flex items-center gap-1.5 rounded border border-line-strong bg-bg-3 px-2 py-1 font-medium text-fg-2 transition-colors hover:bg-bg-5"
            style={{ fontSize: "10.5px" }}
            data-testid={`${testIdPrefix}-sync-ff-retry`}
          >
            <ChevronsRight size={11} />
            Try again
          </button>
        )}
        {fetchAgain}
      </div>
    </>
  );
}

/** `~/…/prompt-driven-orchestrator` — a path the popover can fit on one line. */
function shortPath(path: string, keep = 2): string {
  const parts = path.split("/").filter(Boolean);
  if (parts.length <= keep) return path;
  return `…/${parts.slice(-keep).join("/")}`;
}

function Title({ icon, children }: { icon: React.ReactNode; children: React.ReactNode }) {
  return (
    <div
      className="flex items-start gap-1.5 font-semibold text-fg"
      style={{ fontSize: "11.5px" }}
      data-testid="sync-popover-title"
    >
      <span className="mt-0.5 shrink-0">{icon}</span>
      <span>{children}</span>
    </div>
  );
}

function Body({ children }: { children: React.ReactNode }) {
  return (
    <p className="mt-1.5 leading-snug text-fg-3" style={{ fontSize: "10.5px" }}>
      {children}
    </p>
  );
}

/** "main is 3 commits behind origin/main" — plural-aware, both axes named. */
function gapSentence(
  branchName: string,
  gap: Extract<BranchGap, { kind: "ahead" | "behind" | "diverged" }>,
  upstream: string,
): React.ReactNode {
  const name = <span className="font-mono">{branchName}</span>;
  const up = <span className="font-mono">{upstream}</span>;
  const commits = (n: number) => `${n} commit${n > 1 ? "s" : ""}`;
  if (gap.kind === "behind") {
    return (
      <>
        {name} is <strong>{commits(gap.behind)} behind</strong> {up}
      </>
    );
  }
  if (gap.kind === "ahead") {
    return (
      <>
        {name} is <strong>{commits(gap.ahead)} ahead of</strong> {up}
      </>
    );
  }
  return (
    <>
      {name} has <strong>diverged</strong> from {up} — {gap.ahead} ahead, {gap.behind} behind
    </>
  );
}

/** "Fetched 12 s ago." / "Never fetched." */
function fetchedSentence(lastFetchAt: string | null): string {
  const age = shortAge(lastFetchAt);
  return age ? `Fetched ${age} ago.` : "Never fetched.";
}
