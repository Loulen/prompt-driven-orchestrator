import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  Check,
  ChevronDown,
  GitBranch,
  Cloud,
  CloudOff,
  Laptop,
  RefreshCw,
  Search,
  TriangleAlert,
} from "lucide-react";
import type { BranchFetchError, BranchRef } from "../types";
import {
  branchGap,
  filterBranches,
  gapLabel,
  groupBranches,
  highlightSegments,
  shortAge,
  syncState,
  upstreamSwitchTarget,
  type BranchGap,
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
  /**
   * What the chosen branch is FOR (#804). `"run"` — the default — is a one-shot
   * launch: the form just fetched, so what you see is what you get. `"trigger"`
   * is a template that will fire again and again, which is the only context where
   * a *local* branch is worth a warning: the pre-fire fetch cannot move it, so
   * every fire cuts from wherever that branch was left on disk.
   */
  purpose?: "run" | "trigger";
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
  /** Distinguishes the primary field from each secondary row's. */
  testIdPrefix: string;
  /** `id` of the trigger, so an outer `<label htmlFor>` points at it. */
  id?: string;
  /** Accessible name when there is no visible label (secondary rows). */
  ariaLabel?: string;
}

export default function SourceBranchField({
  purpose = "run",
  value,
  onChange,
  branches,
  loading,
  fetching,
  lastFetchAt,
  fetchError,
  onFetch,
  testIdPrefix,
  id,
  ariaLabel,
}: Props) {
  const [pickerOpen, setPickerOpen] = useState(false);
  const [syncOpen, setSyncOpen] = useState(false);
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
          open={syncOpen}
          onToggle={() => {
            // A click opens the state; it never refetches by itself — the fetch is
            // an action you choose in the popover, having read what it will do.
            setPickerOpen(false);
            setSyncOpen((o) => !o);
          }}
          onFetch={onFetch}
          onSwitchToUpstream={() => {
            const target = upstreamSwitchTarget(selected);
            if (target) {
              onChange(target);
              setSyncOpen(false);
            }
          }}
          upstream={upstreamSwitchTarget(selected)}
          branchName={value}
          purpose={purpose}
          testIdPrefix={testIdPrefix}
        />

        {/* ── #804: the one thing a Trigger's source branch can be wrong about.
            The slot is rendered for EVERY branch in Trigger mode and only filled
            for a local one, so switching selection never shifts the row under the
            cursor. Nothing to click: the escape hatch (`Launch from origin/…`)
            already lives one button to the left, and a second affordance saying
            the same thing would just be another place to disagree with it. */}
        {purpose === "trigger" && (
          <span
            className="flex w-3 shrink-0 self-stretch items-center justify-center"
            data-testid={`${testIdPrefix}-local-warning-slot`}
          >
            {selected?.kind === "local" && (
              <span
                role="img"
                title={localBranchWarning(selected)}
                aria-label={localBranchWarning(selected)}
                data-testid={`${testIdPrefix}-local-warning`}
              >
                <TriangleAlert size={12} className="text-fg-4" aria-hidden />
              </span>
            )}
          </span>
        )}
      </div>
    </div>
  );
}

/**
 * What a local branch means on a Trigger, said plainly (#804, ADR-0070 §1) — an
 * honest reminder, not an error: the choice is legitimate, it just does not mean
 * what "fetch before the cut" might suggest.
 *
 * The recommendation is dropped when the branch tracks nothing, because there
 * would be no `origin/<branch>` to point at — naming one that does not exist
 * would send the reader to a ref the picker cannot offer.
 */
function localBranchWarning(branch: BranchRef): string {
  const base =
    "Local branch — each fire cuts from its local state, which the pre-fire fetch does not move.";
  // The same target the sync popover's shortcut offers, so the two never name
  // different refs.
  const target = upstreamSwitchTarget(branch);
  return target ? `${base} Prefer ${target}.` : base;
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
  open,
  onToggle,
  onFetch,
  onSwitchToUpstream,
  upstream,
  branchName,
  purpose,
  testIdPrefix,
}: {
  state: SyncState;
  open: boolean;
  onToggle: () => void;
  onFetch: () => void;
  onSwitchToUpstream: () => void;
  upstream: string | null;
  branchName: string;
  purpose: "run" | "trigger";
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
        data-sync-state={state.kind}
      >
        {icon}
        {/* The ONLY écart on the form. Icon-only whenever there is none, so the
            button stays quiet until it has something to say. */}
        {gap && (
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
          onFetch={onFetch}
          onSwitchToUpstream={onSwitchToUpstream}
          upstream={upstream}
          branchName={branchName}
          purpose={purpose}
          testIdPrefix={testIdPrefix}
        />
      )}
    </div>
  );
}

function SyncPopover({
  state,
  onFetch,
  onSwitchToUpstream,
  upstream,
  branchName,
  purpose,
  testIdPrefix,
}: {
  state: SyncState;
  onFetch: () => void;
  onSwitchToUpstream: () => void;
  upstream: string | null;
  branchName: string;
  purpose: "run" | "trigger";
  testIdPrefix: string;
}) {
  const fetchAgain = (
    <button
      type="button"
      onClick={onFetch}
      disabled={state.kind === "fetching"}
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
      className="flex items-center gap-1.5 rounded border border-acc-border bg-acc-bg px-2 py-1 font-medium text-acc transition-colors hover:bg-acc/20"
      style={{ fontSize: "10.5px" }}
      data-testid={`${testIdPrefix}-sync-switch`}
    >
      <Cloud size={11} />
      Launch from {upstream}
    </button>
  ) : null;

  return (
    <div
      className="absolute right-0 top-full z-50 mt-1 w-[300px] rounded-md border border-line-strong bg-bg-4 p-3 shadow-xl"
      data-testid={`${testIdPrefix}-sync-popover`}
      data-sync-state={state.kind}
    >
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
          <Title icon={<TriangleAlert size={12} className="text-st-await" />}>
            {gapSentence(branchName, state.gap, state.upstream)}
          </Title>
          <Body>
            {fetchedSentence(state.lastFetchAt)} PDO never pulls into your checkout:
            launch from the upstream ref to start from the fresh commits, or keep{" "}
            <span className="font-mono text-fg-2">{branchName}</span> as is.
          </Body>
          {/* The offer leads, because a branch that is behind is the case the
              whole feature exists for. */}
          <div className="mt-2 flex flex-wrap gap-1.5">
            {launchFromUpstream}
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
          {/* #804: on a Trigger, "as of last fetch" understates it — the fire
              refreshes this ref itself. Said here rather than as a second icon:
              a tracking ref is the RIGHT choice, and the popover is where someone
              already comes to ask what the fire will do about freshness. */}
          <Body>
            {fetchedSentence(state.lastFetchAt)}
            {purpose === "trigger" && (
              <span data-testid={`${testIdPrefix}-sync-trigger-note`}>
                {" "}
                Fetched before the cut on each fire. A failed fetch still fires: the Run cuts
                from the last known state and records the reason.
              </span>
            )}
          </Body>
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

      <div
        className="mt-2 flex items-center justify-between border-t border-line pt-1.5 text-fg-5"
        style={{ fontSize: "9px" }}
      >
        <span>fetch on open · on repo change · here</span>
        <span>fast-forward: #803</span>
      </div>
    </div>
  );
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
