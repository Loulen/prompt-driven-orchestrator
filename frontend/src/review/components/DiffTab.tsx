import { useState, useEffect, useMemo, useRef, useCallback } from "react";
import {
  ChevronRight,
  ChevronDown,
  Archive,
  Check,
  List,
  SquareArrowOutUpRight,
} from "lucide-react";
import { fetchRunStructuredDiff } from "../api/diff";
import { reviewUrl } from "../../run/model/runRefs";
import { pendingCount } from "../model/commentStatus";
import {
  LARGE_DIFF_FILES,
  fileKey,
  defaultCollapsed,
  deliverySignature,
} from "../model/diffTab";
import type { CollapsedFiles } from "../model/diffTab";
import type { RunState } from "../../shared/generated/RunState";
import type { StructuredDiff } from "../../shared/generated/StructuredDiff";
import DiffFileBody from "./DiffFileBody";
import { Badge, Counts, EmptyState, FilePath, MiniBar, Skeleton, StatusLetter } from "./diffTabParts";

/**
 * The Run-level **Diff tab** (#748, ADR-0067; CONTEXT.md § "Relecture de diff").
 *
 * A quick, Warp-like read of the Run diff — fork point → Run tip, `.pdo/`
 * excluded, same bounds as the LOC stat — as a flat transcript: a sticky
 * summary line, a file ledger, then every file as a full-width header followed
 * by its unified hunks. No ref picker, no side-by-side, no settings: the tab's
 * job is to let the operator decide *fast* whether to go to the Review page.
 *
 * It replaces the collapsible Diff section of Info (and the per-node picker it
 * carried — there is no "node diff" surface any more, ADR-0067 §1). The data
 * comes from the structured endpoint; nothing is parsed in the browser.
 *
 * The expanded/collapsed state is keyed by file path and owned by the parent
 * (`PipelineInfoPanel`), so it survives `Info ↔ Diff` and a reload.
 */

interface Props {
  run: RunState;
  /** Collapsed file paths; `null` = never initialised for this Run. */
  collapsed: CollapsedFiles;
  onCollapsedChange: (next: Set<string>) => void;
}

type Load =
  | { kind: "loading" }
  /** `deliverySig`: the Run's delivery signature when this diff was fetched. */
  | { kind: "ready"; diff: StructuredDiff; deliverySig: string }
  | { kind: "error"; message: string };

export default function DiffTab({ run, collapsed, onCollapsedChange }: Props) {
  const isArchived = run.status === "archived";
  const [load, setLoad] = useState<Load>({ kind: "loading" });
  const [reloading, setReloading] = useState(false);
  const [fullFiles, setFullFiles] = useState<Set<string>>(new Set());
  const [reloadTick, setReloadTick] = useState(0);
  const currentSig = deliverySignature(run);
  const changedSinceLoad = load.kind === "ready" && load.deliverySig !== currentSig;

  useEffect(() => {
    // #376: an archived Run's `pdo/run-<id>` branch is deleted at cleanup
    // (ADR-0020) — nothing to fetch; the render says so honestly.
    if (isArchived) return;
    let stale = false;
    const sigAtFetch = deliverySignature(run);
    fetchRunStructuredDiff(run.run_id)
      .then((d) => {
        if (stale) return;
        setLoad({ kind: "ready", diff: d, deliverySig: sigAtFetch });
      })
      .catch((e: unknown) => {
        if (stale) return;
        setLoad({ kind: "error", message: e instanceof Error ? e.message : String(e) });
      })
      .finally(() => {
        if (!stale) setReloading(false);
      });
    return () => {
      stale = true;
    };
    // `run` is deliberately NOT a dependency: a Run state push must not refetch
    // (and lose the reader's place) — the "Diff changed · Reload" affordance is
    // the explicit gesture. `reloadTick` is that gesture.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [run.run_id, isArchived, reloadTick]);

  const files = useMemo(() => (load.kind === "ready" ? load.diff.files : []), [load]);

  // Until the user touches the layout, the default applies (derived, never
  // written back — the parent only stores what the user chose).
  const collapsedSet = useMemo(
    () => collapsed ?? defaultCollapsed(files),
    [collapsed, files],
  );

  const toggleFile = (key: string) => {
    const next = new Set(collapsedSet);
    if (next.has(key)) next.delete(key);
    else next.add(key);
    onCollapsedChange(next);
  };
  const collapseAll = () => onCollapsedChange(new Set(files.map(fileKey)));
  const expandAll = () => onCollapsedChange(new Set(files.filter((f) => f.binary).map(fileKey)));

  // --- scroll tracking: which file is on screen, are we past the ledger -----
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const ledgerRef = useRef<HTMLDivElement | null>(null);
  const headerRefs = useRef<Map<string, HTMLDivElement>>(new Map());
  const [pastLedger, setPastLedger] = useState(false);
  const [currentIdx, setCurrentIdx] = useState(0);

  const onScroll = useCallback(() => {
    const el = scrollRef.current;
    if (!el) return;
    const top = el.scrollTop;
    const ledger = ledgerRef.current;
    const ledgerBottom = ledger ? ledger.offsetTop + ledger.offsetHeight : 0;
    setPastLedger(top > ledgerBottom);
    let idx = 0;
    files.forEach((f, i) => {
      const h = headerRefs.current.get(fileKey(f));
      // 40px: the sticky summary's height — a header tucked under it counts as passed.
      if (h && h.offsetTop <= top + 40) idx = i;
    });
    setCurrentIdx(idx);
  }, [files]);

  const scrollToFile = (key: string) => {
    const h = headerRefs.current.get(key);
    const el = scrollRef.current;
    if (!h || !el) return;
    // The summary line is sticky: land the header just under it.
    el.scrollTo({ top: Math.max(0, h.offsetTop - 36) });
  };
  const scrollToLedger = () => scrollRef.current?.scrollTo({ top: 0 });

  const reload = () => {
    setReloading(true);
    setReloadTick((t) => t + 1);
  };

  // --- states ---------------------------------------------------------------
  if (isArchived) {
    return (
      <EmptyState
        testid="diff-archived"
        icon={<Archive size={14} />}
        title="Diff not preserved for archived runs"
        subtitle="The run branch was deleted at cleanup."
      />
    );
  }

  if (load.kind === "loading") {
    return <Skeleton />;
  }

  if (load.kind === "error") {
    return (
      <EmptyState
        testid="diff-error"
        icon={<Archive size={14} />}
        title="Diff unavailable"
        subtitle={load.message}
        action={
          <button
            onClick={reload}
            className="rounded border border-line-strong bg-surface-2 px-2 py-0.5 text-text-3 transition-colors hover:text-text-2 cursor-pointer"
            style={{ fontSize: "10.5px" }}
          >
            Retry
          </button>
        }
      />
    );
  }

  const diff = load.diff;
  if (diff.files.length === 0) {
    return (
      <EmptyState
        testid="diff-empty"
        icon={<Check size={14} />}
        title="No changes"
        subtitle="Fork point and run tip are identical."
      />
    );
  }

  const isLarge = diff.files.length > LARGE_DIFF_FILES;
  const currentFile = diff.files[currentIdx];

  return (
    <div
      ref={scrollRef}
      onScroll={onScroll}
      data-testid="diff-tab"
      className="flex min-h-0 flex-1 flex-col overflow-y-auto"
    >
      {/* Sticky summary */}
      <div
        className="sticky top-0 z-10 flex h-9 shrink-0 items-center gap-2 border-b border-line bg-surface-1 px-3"
        style={{ fontSize: "11px" }}
        data-testid="diff-summary"
      >
        <span className="text-text-2">
          {diff.files_changed} {diff.files_changed === 1 ? "file" : "files"}
        </span>
        <Counts additions={diff.additions} deletions={diff.deletions} />
        {pastLedger && currentFile ? (
          <span
            className="min-w-0 truncate font-mono text-text-3"
            style={{ fontSize: "10.5px" }}
            data-testid="diff-summary-current"
            title={fileKey(currentFile)}
          >
            {basename(fileKey(currentFile))} · {currentIdx + 1} / {diff.files.length}
          </span>
        ) : (
          <span
            className="min-w-0 truncate font-mono text-text-3"
            style={{ fontSize: "10.5px" }}
            data-testid="diff-summary-refs"
          >
            {shortSha(diff.from_sha ?? diff.from_ref)} → {shortSha(diff.to_sha ?? diff.to_ref)}
          </span>
        )}
        <span className="ml-auto flex shrink-0 items-center gap-1.5">
          {reloading && (
            <span
              className="flex items-center gap-1 rounded-full border border-st-running/40 bg-st-running/14 px-1.5 py-px text-st-running"
              style={{ fontSize: "10px" }}
              data-testid="diff-updating"
            >
              <span className="h-1.5 w-1.5 rounded-full bg-st-running animate-pulse" />
              updating
            </span>
          )}
          {!reloading && changedSinceLoad && (
            <button
              onClick={reload}
              className="flex items-center gap-1 rounded-full border border-st-running/40 bg-st-running/14 px-1.5 py-px text-st-running transition-colors hover:border-st-running cursor-pointer"
              style={{ fontSize: "10px" }}
              data-testid="diff-changed-reload"
            >
              <span className="h-1.5 w-1.5 rounded-full bg-st-running" />
              Diff changed · Reload
            </button>
          )}
          {pastLedger && (
            <button
              onClick={scrollToLedger}
              className="flex items-center gap-1 rounded border border-line-strong bg-surface-2 px-2 py-0.5 text-text-3 transition-colors hover:text-text-2 cursor-pointer"
              style={{ fontSize: "10.5px" }}
              data-testid="diff-files-button"
            >
              <List size={11} />
              Files
            </button>
          )}
          {/* #749: the way to the Review page (the Run's default pair, same tab). */}
          <a
            href={reviewUrl(run.run_id)}
            title="Open the Review page"
            className="flex items-center gap-1 rounded border border-line-strong bg-surface-2 px-2 py-0.5 text-text-3 transition-colors hover:text-text-2"
            style={{ fontSize: "10.5px" }}
            data-testid="diff-review-button"
          >
            <SquareArrowOutUpRight size={11} />
            Expand and comment
            {pendingCount(run.review_comments) > 0 && (
              // #750: pending review comments (sent, not resolved) — CONTEXT.md « Accès rapide Review ».
              <span
                className="ml-0.5 inline-flex h-[14px] items-center rounded-[7px] bg-st-running/14 px-[5px] font-medium text-st-running"
                style={{ fontSize: "9.5px" }}
                title={`${pendingCount(run.review_comments)} review comment(s) pending — sent, not resolved`}
                data-testid="diff-review-pending"
              >
                {pendingCount(run.review_comments)}
              </span>
            )}
          </a>
        </span>
      </div>

      {isLarge && (
        <div
          className="border-b border-line bg-surface-2 px-3 py-1.5 text-text-3"
          style={{ fontSize: "10.5px" }}
          data-testid="diff-large-banner"
        >
          Large diff — files collapsed
        </div>
      )}

      {/* Ledger */}
      <div ref={ledgerRef} className="border-b border-line px-3 py-2" data-testid="diff-ledger">
        <div className="flex flex-col">
          {diff.files.map((f, i) => {
            const key = fileKey(f);
            const isCurrent = pastLedger && i === currentIdx;
            return (
              <button
                key={key}
                onClick={() => scrollToFile(key)}
                className={`flex items-center gap-2 rounded px-1 py-px text-left transition-colors hover:bg-hover cursor-pointer ${
                  isCurrent ? "bg-surface-2" : ""
                }`}
                style={{ fontSize: "10.5px" }}
                data-testid="diff-ledger-row"
                title={key}
              >
                <StatusLetter status={f.status} />
                <span className="min-w-0 flex-1 truncate font-mono text-text-2">{key}</span>
                <Counts additions={f.additions} deletions={f.deletions} />
                <MiniBar additions={f.additions} deletions={f.deletions} />
              </button>
            );
          })}
        </div>
        <div className="mt-1.5 flex items-center gap-1 text-text-3" style={{ fontSize: "10.5px" }}>
          <button
            onClick={collapseAll}
            className="hover:text-text-2 cursor-pointer"
            data-testid="diff-collapse-all"
          >
            Collapse all
          </button>
          <span>·</span>
          <button
            onClick={expandAll}
            className="hover:text-text-2 cursor-pointer"
            data-testid="diff-expand-all"
          >
            Expand all
          </button>
        </div>
      </div>

      {/* Flat transcript */}
      {diff.files.map((f) => {
        const key = fileKey(f);
        const isCollapsed = collapsedSet.has(key);
        return (
          <div key={key} data-testid="diff-file" data-path={key} data-collapsed={isCollapsed}>
            <div
              ref={(el) => {
                if (el) headerRefs.current.set(key, el);
                else headerRefs.current.delete(key);
              }}
            >
              <button
                onClick={() => toggleFile(key)}
                className="flex w-full items-center gap-1.5 border-b border-line px-3 py-1.5 text-left transition-colors hover:bg-hover cursor-pointer"
                style={{ fontSize: "10.5px" }}
                data-testid="diff-file-header"
                aria-expanded={!isCollapsed}
              >
                {isCollapsed ? (
                  <ChevronRight size={11} className="shrink-0 text-text-3" />
                ) : (
                  <ChevronDown size={11} className="shrink-0 text-text-3" />
                )}
                <FilePath file={f} />
                <Badge file={f} />
                <span className="ml-auto shrink-0">
                  <Counts additions={f.additions} deletions={f.deletions} />
                </span>
              </button>
            </div>
            {!isCollapsed && !f.binary && (
              <DiffFileBody
                file={f}
                reviewHref={reviewUrl(run.run_id)}
                full={fullFiles.has(key)}
                onShowAll={() => setFullFiles((prev) => new Set(prev).add(key))}
              />
            )}
          </div>
        );
      })}
    </div>
  );
}

function basename(p: string): string {
  const i = p.lastIndexOf("/");
  return i < 0 ? p : p.slice(i + 1);
}

function shortSha(s: string): string {
  return /^[0-9a-f]{40}$/i.test(s) ? s.slice(0, 7) : s;
}
