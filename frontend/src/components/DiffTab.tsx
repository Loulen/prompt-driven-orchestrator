import { useState, useEffect, useMemo, useRef, useCallback, Fragment } from "react";
import type { ReactNode } from "react";
import {
  ChevronRight,
  ChevronDown,
  Archive,
  Check,
  List,
  SquareArrowOutUpRight,
} from "lucide-react";
import { fetchRunStructuredDiff } from "../api";
import { wordDiff } from "../lib/wordDiff";
import { reviewUrl } from "../lib/runRefs";
import { pendingCount } from "../lib/reviewComments";
import {
  LARGE_DIFF_FILES,
  TRUNCATE_LINES,
  fileKey,
  defaultCollapsed,
  deliverySignature,
} from "../lib/diffTab";
import type { CollapsedFiles } from "../lib/diffTab";
import type { RunState, StructuredDiff, DiffFile, DiffLine } from "../types";

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
            className="rounded border border-line-strong bg-bg-3 px-2 py-0.5 text-fg-3 transition-colors hover:text-fg-2 cursor-pointer"
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
        className="sticky top-0 z-10 flex h-9 shrink-0 items-center gap-2 border-b border-line bg-bg-2 px-3"
        style={{ fontSize: "11px" }}
        data-testid="diff-summary"
      >
        <span className="text-fg-2">
          {diff.files_changed} {diff.files_changed === 1 ? "file" : "files"}
        </span>
        <Counts additions={diff.additions} deletions={diff.deletions} />
        {pastLedger && currentFile ? (
          <span
            className="min-w-0 truncate font-mono text-fg-4"
            style={{ fontSize: "10.5px" }}
            data-testid="diff-summary-current"
            title={fileKey(currentFile)}
          >
            {basename(fileKey(currentFile))} · {currentIdx + 1} / {diff.files.length}
          </span>
        ) : (
          <span
            className="min-w-0 truncate font-mono text-fg-4"
            style={{ fontSize: "10.5px" }}
            data-testid="diff-summary-refs"
          >
            {shortSha(diff.from_sha ?? diff.from_ref)} → {shortSha(diff.to_sha ?? diff.to_ref)}
          </span>
        )}
        <span className="ml-auto flex shrink-0 items-center gap-1.5">
          {reloading && (
            <span
              className="flex items-center gap-1 rounded-full border border-st-running/40 bg-st-running-bg px-1.5 py-px text-st-running"
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
              className="flex items-center gap-1 rounded-full border border-st-running/40 bg-st-running-bg px-1.5 py-px text-st-running transition-colors hover:border-st-running cursor-pointer"
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
              className="flex items-center gap-1 rounded border border-line-strong bg-bg-3 px-2 py-0.5 text-fg-3 transition-colors hover:text-fg-2 cursor-pointer"
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
            className="flex items-center gap-1 rounded border border-line-strong bg-bg-3 px-2 py-0.5 text-fg-3 transition-colors hover:text-fg-2"
            style={{ fontSize: "10.5px" }}
            data-testid="diff-review-button"
          >
            <SquareArrowOutUpRight size={11} />
            Expand and comment
            {pendingCount(run.review_comments) > 0 && (
              // #750: pending review comments (sent, not resolved) — CONTEXT.md « Accès rapide Review ».
              <span
                className="ml-0.5 inline-flex h-[14px] items-center rounded-[7px] bg-st-running-bg px-[5px] font-medium text-st-running"
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
          className="border-b border-line bg-bg-3 px-3 py-1.5 text-fg-3"
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
                className={`flex items-center gap-2 rounded px-1 py-px text-left transition-colors hover:bg-bg-3 cursor-pointer ${
                  isCurrent ? "bg-bg-3" : ""
                }`}
                style={{ fontSize: "10.5px" }}
                data-testid="diff-ledger-row"
                title={key}
              >
                <StatusLetter status={f.status} />
                <span className="min-w-0 flex-1 truncate font-mono text-fg-2">{key}</span>
                <Counts additions={f.additions} deletions={f.deletions} />
                <MiniBar additions={f.additions} deletions={f.deletions} />
              </button>
            );
          })}
        </div>
        <div className="mt-1.5 flex items-center gap-1 text-fg-4" style={{ fontSize: "10.5px" }}>
          <button
            onClick={collapseAll}
            className="hover:text-fg-2 cursor-pointer"
            data-testid="diff-collapse-all"
          >
            Collapse all
          </button>
          <span>·</span>
          <button
            onClick={expandAll}
            className="hover:text-fg-2 cursor-pointer"
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
                className="flex w-full items-center gap-1.5 border-b border-line px-3 py-1.5 text-left transition-colors hover:bg-bg-3 cursor-pointer"
                style={{ fontSize: "10.5px" }}
                data-testid="diff-file-header"
                aria-expanded={!isCollapsed}
              >
                {isCollapsed ? (
                  <ChevronRight size={11} className="shrink-0 text-fg-4" />
                ) : (
                  <ChevronDown size={11} className="shrink-0 text-fg-4" />
                )}
                <FilePath file={f} />
                <Badge file={f} />
                <span className="ml-auto shrink-0">
                  <Counts additions={f.additions} deletions={f.deletions} />
                </span>
              </button>
            </div>
            {!isCollapsed && !f.binary && (
              <FileBody
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

// ---------------------------------------------------------------------------

function EmptyState({
  testid,
  icon,
  title,
  subtitle,
  action,
}: {
  testid: string;
  icon: ReactNode;
  title: string;
  subtitle: string;
  action?: ReactNode;
}) {
  return (
    <div
      className="flex flex-1 flex-col items-center justify-center gap-2 px-6 py-10 text-center"
      data-testid={testid}
    >
      <span className="grid h-7 w-7 place-items-center rounded border border-line bg-bg-3 text-fg-3">
        {icon}
      </span>
      <div className="text-fg-3" style={{ fontSize: "11.5px" }}>
        {title}
      </div>
      <div className="text-fg-4" style={{ fontSize: "10.5px" }}>
        {subtitle}
      </div>
      {action}
    </div>
  );
}

function Skeleton() {
  return (
    <div className="flex flex-col gap-2 px-3 py-3" data-testid="diff-loading" aria-busy>
      <div className="h-3 w-24 animate-pulse rounded bg-bg-4" />
      <div className="mt-1 h-px bg-line" />
      <div className="h-3 w-3/4 animate-pulse rounded bg-bg-3" />
      <div className="h-3 w-full animate-pulse rounded bg-bg-3" />
      <div className="h-3 w-2/3 animate-pulse rounded bg-bg-3" />
    </div>
  );
}

function Counts({ additions, deletions }: { additions: number; deletions: number }) {
  return (
    <span className="shrink-0 font-mono tabular-nums" style={{ fontSize: "10.5px" }}>
      <span className="text-st-done">+{additions}</span>{" "}
      <span className="text-st-failed">−{deletions}</span>
    </span>
  );
}

/** Five blocks, green then red, proportional to the file's +/- share. */
function MiniBar({ additions, deletions }: { additions: number; deletions: number }) {
  const total = additions + deletions;
  const green = total === 0 ? 0 : Math.round((additions / total) * 5);
  return (
    <span className="flex shrink-0 gap-px" aria-hidden>
      {Array.from({ length: 5 }, (_, i) => (
        <span
          key={i}
          className={`h-2 w-1.5 rounded-[1px] ${
            total === 0 ? "bg-bg-4" : i < green ? "bg-st-done" : "bg-st-failed"
          }`}
        />
      ))}
    </span>
  );
}

const STATUS_LETTER: Record<DiffFile["status"], { letter: string; className: string }> = {
  modified: { letter: "M", className: "text-st-await" },
  added: { letter: "A", className: "text-st-done" },
  deleted: { letter: "D", className: "text-st-failed" },
  renamed: { letter: "R", className: "text-st-running" },
  copied: { letter: "C", className: "text-st-running" },
};

function StatusLetter({ status }: { status: DiffFile["status"] }) {
  const s = STATUS_LETTER[status];
  return (
    <span className={`w-2.5 shrink-0 text-center font-mono font-medium ${s.className}`}>
      {s.letter}
    </span>
  );
}

function basename(p: string): string {
  const i = p.lastIndexOf("/");
  return i < 0 ? p : p.slice(i + 1);
}

function shortSha(s: string): string {
  return /^[0-9a-f]{40}$/i.test(s) ? s.slice(0, 7) : s;
}

/** Directory dimmed, file name in clear; `old → new` for a rename/copy. */
function FilePath({ file }: { file: DiffFile }) {
  const isMove = file.status === "renamed" || file.status === "copied";
  const key = fileKey(file);
  const i = key.lastIndexOf("/");
  const dir = i < 0 ? "" : key.slice(0, i + 1);
  const name = i < 0 ? key : key.slice(i + 1);
  return (
    <span className="min-w-0 truncate font-mono" title={isMove ? `${file.old_path} → ${file.new_path}` : key}>
      {isMove && file.old_path && (
        <>
          <span className="text-fg-4">{file.old_path}</span>
          <span className="text-fg-4"> → </span>
        </>
      )}
      <span className="text-fg-4">{dir}</span>
      <span className="text-fg-2">{name || "(unknown path)"}</span>
    </span>
  );
}

function Badge({ file }: { file: DiffFile }) {
  const label = file.binary
    ? "binary"
    : file.status === "added"
      ? "new"
      : file.status === "deleted"
        ? "deleted"
        : file.status === "renamed"
          ? "renamed"
          : file.status === "copied"
            ? "copied"
            : null;
  if (!label) return null;
  const tone =
    label === "new"
      ? "border-st-done/40 text-st-done"
      : label === "deleted"
        ? "border-st-failed/40 text-st-failed"
        : "border-line-strong text-fg-4";
  return (
    <span
      className={`shrink-0 rounded border px-1 leading-4 ${tone}`}
      style={{ fontSize: "9.5px" }}
      data-testid="diff-file-badge"
    >
      {label}
    </span>
  );
}

// ---------------------------------------------------------------------------
// File body: unified hunks with a sticky old/new gutter, intra-line word
// highlight on adjacent `-`/`+` runs, truncation past TRUNCATE_LINES.
// ---------------------------------------------------------------------------

type Row =
  | { kind: "hunk"; header: string; text: string }
  | { kind: "line"; line: DiffLine; segs: { text: string; changed: boolean }[] | null };

function buildRows(file: DiffFile): Row[] {
  const rows: Row[] = [];
  for (const h of file.hunks) {
    rows.push({
      kind: "hunk",
      header: h.header,
      text: `@@ -${h.old_start},${h.old_lines} +${h.new_start},${h.new_lines} @@`,
    });
    // Pair each run of `-` lines with the run of `+` lines that follows it,
    // index-wise (GitHub's rule), and word-diff each pair.
    const lines = h.lines;
    let i = 0;
    while (i < lines.length) {
      if (lines[i].kind !== "del") {
        rows.push({ kind: "line", line: lines[i], segs: null });
        i++;
        continue;
      }
      let j = i;
      while (j < lines.length && lines[j].kind === "del") j++;
      let k = j;
      while (k < lines.length && lines[k].kind === "add") k++;
      const dels = lines.slice(i, j);
      const adds = lines.slice(j, k);
      const pairs = Math.min(dels.length, adds.length);
      const delSegs: (Row & { kind: "line" })["segs"][] = dels.map(() => null);
      const addSegs: (Row & { kind: "line" })["segs"][] = adds.map(() => null);
      for (let p = 0; p < pairs; p++) {
        const d = wordDiff(dels[p].content, adds[p].content);
        // Highlight only when something is shared: an entirely different line
        // painted red/green end to end says nothing the row colour does not.
        const shared = d.old.some((s) => !s.changed) || d.new.some((s) => !s.changed);
        if (shared) {
          delSegs[p] = d.old;
          addSegs[p] = d.new;
        }
      }
      dels.forEach((l, p) => rows.push({ kind: "line", line: l, segs: delSegs[p] }));
      adds.forEach((l, p) => rows.push({ kind: "line", line: l, segs: addSegs[p] }));
      i = k;
    }
  }
  return rows;
}

function FileBody({
  file,
  reviewHref,
  full,
  onShowAll,
}: {
  file: DiffFile;
  /** The Review page, where context expansion lives (#749). */
  reviewHref: string;
  full: boolean;
  onShowAll: () => void;
}) {
  const rows = useMemo(() => buildRows(file), [file]);
  const lineRows = rows.filter((r) => r.kind === "line").length;
  const truncated = !full && lineRows > TRUNCATE_LINES;
  let shown = rows;
  if (truncated) {
    let count = 0;
    let cut = rows.length;
    for (let i = 0; i < rows.length; i++) {
      if (rows[i].kind === "line") count++;
      if (count >= TRUNCATE_LINES) {
        cut = i + 1;
        break;
      }
    }
    shown = rows.slice(0, cut);
  }
  const hidden = lineRows - TRUNCATE_LINES;

  return (
    <div className="border-b border-line" data-testid="diff-file-body">
      <div className="overflow-x-auto">
        <table
          className="w-max min-w-full border-collapse font-mono select-text"
          style={{ fontSize: "10.5px", lineHeight: "18px" }}
        >
          <tbody>
            {shown.map((r, idx) => {
              if (r.kind === "hunk") {
                return (
                  <tr key={idx} className="bg-bg-3/60 text-fg-4" data-testid="diff-hunk">
                    <td
                      colSpan={2}
                      className="sticky left-0 z-[1] bg-bg-3 px-2 text-right select-none"
                      aria-hidden
                    />
                    <td className="whitespace-pre px-2 text-acc/70" colSpan={2}>
                      <span className="flex items-center gap-2">
                        <span>
                          {r.text}
                          {r.header && <span className="text-fg-4"> {r.header}</span>}
                        </span>
                        {/* Context expansion lives on the Review page (#749). */}
                        <a
                          href={reviewHref}
                          className="ml-auto pl-4 text-fg-5 hover:text-fg-3"
                          title="Expand context on the Review page"
                          tabIndex={-1}
                        >
                          ↕ expand
                        </a>
                      </span>
                    </td>
                  </tr>
                );
              }
              const { line, segs } = r;
              const rowTone =
                line.kind === "add"
                  ? "bg-st-done-bg"
                  : line.kind === "del"
                    ? "bg-st-failed-bg"
                    : "";
              const gutterTone =
                line.kind === "add"
                  ? "bg-st-done-bg"
                  : line.kind === "del"
                    ? "bg-st-failed-bg"
                    : "bg-bg-2";
              const marker = line.kind === "add" ? "+" : line.kind === "del" ? "−" : " ";
              const markerTone =
                line.kind === "add"
                  ? "text-st-done"
                  : line.kind === "del"
                    ? "text-st-failed"
                    : "text-fg-5";
              return (
                <tr key={idx} className={rowTone} data-testid={`diff-line-${line.kind}`}>
                  <td
                    className={`sticky left-0 z-[1] w-8 min-w-8 pl-2 pr-1 text-right text-fg-5 select-none ${gutterTone}`}
                    aria-hidden
                  >
                    {line.old_no ?? ""}
                  </td>
                  <td
                    className={`sticky left-8 z-[1] w-8 min-w-8 pr-2 text-right text-fg-5 select-none ${gutterTone}`}
                    aria-hidden
                  >
                    {line.new_no ?? ""}
                  </td>
                  <td className={`w-3 pl-2 select-none ${markerTone}`} aria-hidden>
                    {marker}
                  </td>
                  <td className={`whitespace-pre pr-4 ${line.kind === "context" ? "text-fg-3" : "text-fg"}`}>
                    {segs ? <Highlighted segs={segs} kind={line.kind} /> : line.content}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
      {truncated && (
        <button
          onClick={onShowAll}
          className="w-full py-1.5 text-center text-acc transition-colors hover:bg-bg-3 cursor-pointer"
          style={{ fontSize: "10.5px" }}
          data-testid="diff-show-more"
        >
          Show {hidden} more lines
        </button>
      )}
    </div>
  );
}

function Highlighted({
  segs,
  kind,
}: {
  segs: { text: string; changed: boolean }[];
  kind: DiffLine["kind"];
}) {
  const tone = kind === "add" ? "bg-st-done/35" : "bg-st-failed/35";
  return (
    <>
      {segs.map((s, i) => (
        <Fragment key={i}>
          {s.changed ? (
            <span className={`rounded-[2px] ${tone}`} data-testid="diff-word">
              {s.text}
            </span>
          ) : (
            s.text
          )}
        </Fragment>
      ))}
    </>
  );
}
