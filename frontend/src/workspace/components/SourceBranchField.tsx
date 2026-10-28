import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ChevronDown, TriangleAlert } from "lucide-react";
import type { BranchFetchError } from "../../shared/generated/BranchFetchError";
import type { BranchRef } from "../../shared/generated/BranchRef";
import type { FastForwardOutcome } from "../model/branches";
import {
  canFastForward,
  fastForwardTouchesCheckout,
  filterBranches,
  groupBranches,
  shortAge,
  syncState,
  upstreamSwitchTarget,
} from "../model/branchSelect";
import { useFastForwardCard } from "../store/useFastForwardCard";
import { BranchPicker, KindIcon } from "./BranchPicker";
import { SyncButton } from "./SyncButton";

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
  /**
   * Re-read the list from LOCAL refs — no network (#803).
   *
   * Called when the sync popover opens, and nowhere else. The écart the popover
   * shows now decides whether a BUTTON THAT ACTS is offered, so it may not be a
   * number cached since the modal opened: commit in a terminal while the form sits
   * there and the cached list still says "1 behind, 0 ahead", still offering a
   * fast-forward on a branch that has already diverged. Re-reading refs is local
   * and cheap; it is not the fetch the footer promises to do only on open, on repo
   * change and on the button — no remote is contacted here.
   *
   * Optional: a call site with nothing to re-read simply never refreshes.
   */
  onReread?: () => void;
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
  purpose = "run",
  value,
  onChange,
  branches,
  loading,
  fetching,
  lastFetchAt,
  fetchError,
  onFetch,
  onReread,
  onFastForward,
  testIdPrefix,
  id,
  ariaLabel,
}: Props) {
  const [pickerOpen, setPickerOpen] = useState(false);
  const [syncOpen, setSyncOpen] = useState(false);
  const { ffState, runFastForward, fetchAndRetire, newOpening } = useFastForwardCard({
    syncOpen,
    value,
    onFastForward,
    onFetch,
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
            className={`flex w-full items-center gap-2 rounded-md border bg-surface-2 px-2.5 py-1.5 text-left font-mono text-text transition-colors focus:outline-none disabled:opacity-40 ${
              pickerOpen ? "border-primary" : "border-line-strong hover:border-text-3"
            }`}
            style={{ fontSize: "12px" }}
            data-testid={`${testIdPrefix}-trigger`}
          >
            <KindIcon kind={selected?.kind} />
            <span className="min-w-0 flex-1 truncate">
              {value || (triggerDisabled ? "Loading branches…" : "Select a branch")}
            </span>
            {selectedAge && (
              <span className="shrink-0 text-text-3" style={{ fontSize: "11px" }}>
                {selectedAge}
              </span>
            )}
            <ChevronDown size={12} className="shrink-0 text-text-3" />
          </button>

          {pickerOpen && (
            <BranchPicker
              query={query}
              onQueryChange={(q) => {
                setQuery(q);
                setCursor(0);
              }}
              onSearchKeyDown={onSearchKeyDown}
              visibleLocals={visibleLocals}
              visibleRemotes={visibleRemotes}
              localCount={locals.length}
              remoteCount={remotes.length}
              cursor={cursorInRange}
              onHover={setCursor}
              value={value}
              onPick={pick}
              lastFetchAt={lastFetchAt}
              fetchError={fetchError}
              fetching={fetching}
              testIdPrefix={testIdPrefix}
            />
          )}
        </div>

        {/* ── The sync button: the ONE place the selection's écart is shown. ── */}
        <SyncButton
          state={sync}
          ff={ffState}
          open={syncOpen}
          onToggle={() => {
            // A click opens the state; it never refetches by itself — the fetch is
            // an action you choose in the popover, having read what it will do. It
            // does RE-READ the local refs, which is a different thing entirely: no
            // network, and it is what keeps the fast-forward button from being
            // offered for a branch that diverged while the form sat open.
            setPickerOpen(false);
            if (!syncOpen) onReread?.();
            setSyncOpen((o) => !o);
            // A new opening retires the previous card (see `useFastForwardCard`).
            newOpening();
          }}
          onFetch={fetchAndRetire}
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
                <TriangleAlert size={12} className="text-text-3" aria-hidden />
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
