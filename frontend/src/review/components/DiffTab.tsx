import { useState, useEffect, useMemo, useRef, useCallback } from "react";
import {
  Archive,
  Check,
  ChevronDown,
  ChevronRight,
  CircleAlert,
  List,
  ListCollapse,
  ListTree,
  RefreshCw,
  SquareArrowOutUpRight,
} from "lucide-react";
import { Button, Chip, EmptyState, IconButton } from "../../shared/ui/kit";
import "../../pipeline/components/inspect.css";
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
import { Badge, Counts, FilePath, Skeleton, StatusIcon, StatusLetter } from "./diffTabParts";

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
      // 32px: the sticky summary's height and a little — a header tucked under it counts as passed.
      if (h && h.offsetTop <= top + 32) idx = i;
    });
    setCurrentIdx(idx);
  }, [files]);

  const scrollToFile = (key: string) => {
    const h = headerRefs.current.get(key);
    const el = scrollRef.current;
    if (!h || !el) return;
    // The summary line is sticky: land the header just under it.
    el.scrollTo({ top: Math.max(0, h.offsetTop - 28) });
  };
  const scrollToLedger = () => scrollRef.current?.scrollTo({ top: 0 });

  const reload = () => {
    setReloading(true);
    setReloadTick((t) => t + 1);
  };

  // --- states ---------------------------------------------------------------
  if (isArchived) {
    return (
      <div className="pi-center" data-testid="diff-archived" title="The run branch was deleted at cleanup">
        <EmptyState icon={Archive}>Diff not preserved</EmptyState>
      </div>
    );
  }

  if (load.kind === "loading") {
    return <Skeleton />;
  }

  if (load.kind === "error") {
    return (
      <div className="pi-center" data-testid="diff-error">
        <EmptyState
          icon={CircleAlert}
          action={
            <Button size="sm" onClick={reload}>
              Retry
            </Button>
          }
        >
          <span title={load.message}>Diff unavailable</span>
        </EmptyState>
      </div>
    );
  }

  const diff = load.diff;
  if (diff.files.length === 0) {
    return (
      <div className="pi-center" data-testid="diff-empty" title="Fork point and run tip are identical">
        <EmptyState icon={Check}>No changes</EmptyState>
      </div>
    );
  }

  const isLarge = diff.files.length > LARGE_DIFF_FILES;
  const currentFile = diff.files[currentIdx];
  const pending = pendingCount(run.review_comments);

  return (
    <div ref={scrollRef} onScroll={onScroll} data-testid="diff-tab" className="pi-diff">
      {/* Sticky summary */}
      <div className="pi-diffhead" data-testid="diff-summary">
        <Counts additions={diff.additions} deletions={diff.deletions} />
        <Chip>
          {diff.files_changed} {diff.files_changed === 1 ? "file" : "files"}
        </Chip>
        {pastLedger && currentFile ? (
          <span className="d-mono ei-meta ei-trunc" data-testid="diff-summary-current" title={fileKey(currentFile)}>
            {basename(fileKey(currentFile))} · {currentIdx + 1} / {diff.files.length}
          </span>
        ) : (
          <span className="d-mono ei-meta ei-trunc" data-testid="diff-summary-refs" title={`${diff.from_ref} → ${diff.to_ref}`}>
            {shortSha(diff.from_sha ?? diff.from_ref)} → {shortSha(diff.to_sha ?? diff.to_ref)}
          </span>
        )}
        <span className="pi-diffhead__end">
          {isLarge && (
            <span title="Large diff: files collapsed" data-testid="diff-large-banner">
              <Chip>collapsed</Chip>
            </span>
          )}
          {reloading && (
            <span data-testid="diff-updating">
              <Chip tone="running">updating</Chip>
            </span>
          )}
          {!reloading && changedSinceLoad && (
            <Button size="sm" tone="ghost" icon={RefreshCw} onClick={reload} data-testid="diff-changed-reload">
              Diff changed · Reload
            </Button>
          )}
          {pastLedger && (
            <IconButton icon={List} size="sm" label="Files" onClick={scrollToLedger} data-testid="diff-files-button" />
          )}
          <IconButton
            icon={ListCollapse}
            size="sm"
            label="Collapse all"
            onClick={collapseAll}
            data-testid="diff-collapse-all"
          />
          <IconButton
            icon={ListTree}
            size="sm"
            label="Expand all"
            onClick={expandAll}
            data-testid="diff-expand-all"
          />
          {/* #749: the way to the Review page (the Run's default pair, same tab). */}
          <a
            href={reviewUrl(run.run_id)}
            title={pending > 0 ? `Review page · ${pending} comment(s) pending` : "Review page"}
            aria-label="Review page"
            className="d-icon"
            data-testid="diff-review-button"
          >
            <SquareArrowOutUpRight size={14} strokeWidth={1.5} aria-hidden />
            {pending > 0 && (
              // #750: pending review comments (sent, not resolved) — CONTEXT.md « Accès rapide Review ».
              <span className="d-icon__badge" data-testid="diff-review-pending">
                {pending}
              </span>
            )}
          </a>
        </span>
      </div>


      {/* Ledger: one SCM row per file */}
      <div ref={ledgerRef} className="pi-dfiles" data-testid="diff-ledger">
        {diff.files.map((f, i) => {
          const key = fileKey(f);
          return (
            <button
              key={key}
              type="button"
              onClick={() => scrollToFile(key)}
              className="pi-dfile"
              aria-current={pastLedger && i === currentIdx ? "true" : undefined}
              data-testid="diff-ledger-row"
              title={key}
            >
              <StatusIcon status={f.status} />
              <FilePath file={f} />
              <Counts additions={f.additions} deletions={f.deletions} />
              <StatusLetter status={f.status} />
            </button>
          );
        })}
      </div>

      {/* Flat transcript */}
      {diff.files.map((f) => {
        const key = fileKey(f);
        const isCollapsed = collapsedSet.has(key);
        const isMove = f.status === "renamed" || f.status === "copied";
        return (
          <div key={key} data-testid="diff-file" data-path={key} data-collapsed={isCollapsed}>
            <div
              ref={(el) => {
                if (el) headerRefs.current.set(key, el);
                else headerRefs.current.delete(key);
              }}
            >
              <button
                type="button"
                onClick={() => toggleFile(key)}
                className="pi-dfile pi-dfile__head"
                title={isMove ? `${f.old_path} → ${f.new_path}` : key}
                data-testid="diff-file-header"
                aria-expanded={!isCollapsed}
              >
                {isCollapsed ? (
                  <ChevronRight size={14} strokeWidth={1.5} aria-hidden className="ei-ic" />
                ) : (
                  <ChevronDown size={14} strokeWidth={1.5} aria-hidden className="ei-ic" />
                )}
                <FilePath file={f} />
                <Badge file={f} />
                <Counts additions={f.additions} deletions={f.deletions} />
                <StatusLetter status={f.status} />
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
