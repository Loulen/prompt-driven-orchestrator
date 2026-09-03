import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Check, Download, Folder, FolderOpen, GitBranch, HardDrive, Pencil, Search, X } from "lucide-react";
import {
  ApiError,
  cancelSkillScan,
  fetchRecentSkillSources,
  importSkills,
  scanSkillSource,
} from "../api";
import type {
  RecentSkillSource,
  SkillCandidate,
  SkillFolder,
  SkillImportItem,
  SkillImportReport,
  SkillScanResult,
} from "../types";
import { timeAgo } from "../lib/skillMd";
import { displaySourceUrl, parseSkillSource, shortCommit } from "../lib/skillSource";
import { folderPathLabel } from "../lib/skillTree";
import FsExplorerModal from "./FsExplorerModal";

interface Props {
  folders: SkillFolder[];
  /** Current bank labels, for the live rename check. */
  existingNames: string[];
  /** Parent folder pre-selected when the popup opened (the tree's selection). */
  initialFolderId: string | null;
  /** Host `$HOME`, to shorten local paths in the recent list. */
  home?: string | null;
  onClose: () => void;
  /**
   * Called after an import wrote something. `complete` is false on a partial
   * failure: the popup stays open with the failed rows in red.
   */
  onImported: (report: SkillImportReport, complete: boolean) => void | Promise<void>;
}

type Step = "source" | "scanning" | "results" | "importing";

type Resolution = "replace" | "rename" | "skip";

interface RowState {
  checked: boolean;
  resolution: Resolution | null;
  renameTo: string;
}

interface ScanError {
  code: string;
  message: string;
}

const HOW_IT_WORKS = [
  "Point at a source",
  "Scan finds every SKILL.md",
  "Check the skills to import",
  "They land in a folder named after the source",
];

function newScanId(): string {
  const c = globalThis.crypto as Crypto | undefined;
  if (c && typeof c.randomUUID === "function") return c.randomUUID();
  return `scan-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
}

function relativise(path: string, home: string | null | undefined): string {
  if (home && path.startsWith(home + "/")) return "~" + path.slice(home.length);
  return path;
}

/** Rebuild the text a recent source was typed as, for the field. */
function recentToText(source: RecentSkillSource): string {
  if (/^https?:\/\//.test(source.url) && (source.ref || source.path)) {
    return `${source.url}/tree/${source.ref ?? "main"}/${source.path}`.replace(/\/$/, "");
  }
  return source.url;
}

/** `git@github.com:owner/repo.git` for a GitHub-like https URL, for the callout. */
function sshHint(url: string): string | null {
  const match = /^https?:\/\/([^/]+)\/([^/]+)\/([^/]+?)(?:\.git)?$/.exec(url);
  return match ? `git@${match[1]}:${match[2]}/${match[3]}.git` : null;
}

function defaultRow(candidate: SkillCandidate): RowState {
  return {
    checked: candidate.status === "new" || candidate.status === "name_taken",
    resolution: null,
    renameTo: "",
  };
}

/**
 * "Import skills from a source" (#670): one field for a GitHub URL (root,
 * branch, `/tree/<branch>/<path>`), an SSH URL, or a local folder; the daemon
 * clones shallow and scans; the results are a checkable list where a taken
 * name must be resolved (replace / rename / skip) before Import enables.
 * Nothing touches the bank before Import; a clone refusal reads in place with
 * git's own message, never as a modal over this modal.
 */
export default function ImportSkillsModal({
  folders,
  existingNames,
  initialFolderId,
  home = null,
  onClose,
  onImported,
}: Props) {
  const [step, setStep] = useState<Step>("source");
  const [text, setText] = useState("");
  const [parentId, setParentId] = useState<string | null>(initialFolderId);
  const [folderName, setFolderName] = useState("");
  const [editingName, setEditingName] = useState(false);
  const [recent, setRecent] = useState<RecentSkillSource[]>([]);
  const [browsing, setBrowsing] = useState(false);
  const [scan, setScan] = useState<SkillScanResult | null>(null);
  const [scanError, setScanError] = useState<ScanError | null>(null);
  const [rows, setRows] = useState<Map<string, RowState>>(new Map());
  const [filter, setFilter] = useState("");
  const [failedRows, setFailedRows] = useState<Map<string, string>>(new Map());
  const [doneRows, setDoneRows] = useState<Set<string>>(new Set());
  const [importError, setImportError] = useState<string | null>(null);
  const [confirmDiscard, setConfirmDiscard] = useState(false);
  const [touched, setTouched] = useState(false);
  const scanIdRef = useRef<string | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  const parsed = useMemo(() => parseSkillSource(text), [text]);

  useEffect(() => {
    inputRef.current?.focus();
    fetchRecentSkillSources()
      .then((result) => setRecent(Array.isArray(result.sources) ? result.sources : []))
      .catch(() => setRecent([]));
  }, []);

  // ---- scan ---------------------------------------------------------------

  const runScan = useCallback(
    async (source: string) => {
      const id = newScanId();
      scanIdRef.current = id;
      setStep("scanning");
      setScanError(null);
      setScan(null);
      setFailedRows(new Map());
      setDoneRows(new Set());
      setImportError(null);
      try {
        const result = await scanSkillSource(id, source);
        if (scanIdRef.current !== id) return; // cancelled or superseded
        setScan(result);
        setRows(new Map(result.candidates.map((c) => [c.path, defaultRow(c)])));
        setFolderName(result.source.suggested_folder);
        setStep("results");
      } catch (cause) {
        if (scanIdRef.current !== id) return;
        const code =
          cause instanceof ApiError && typeof (cause.body as { code?: unknown } | null)?.code === "string"
            ? ((cause.body as { code: string }).code)
            : "";
        setScanError({
          code,
          message: cause instanceof Error ? cause.message : "Scan failed",
        });
        setStep("source");
      }
    },
    [],
  );

  const cancelScan = useCallback(() => {
    const id = scanIdRef.current;
    scanIdRef.current = null;
    if (id) void cancelSkillScan(id).catch(() => undefined);
    setStep("source");
  }, []);

  const startScan = () => {
    if (!parsed || step === "scanning") return;
    void runScan(text);
  };

  const changeSource = () => {
    setScan(null);
    setStep("source");
    setTouched(false);
    window.setTimeout(() => inputRef.current?.focus(), 0);
  };

  // ---- selection ----------------------------------------------------------

  const candidates = useMemo(() => scan?.candidates ?? [], [scan]);
  const setRow = (path: string, patch: Partial<RowState>) => {
    setTouched(true);
    setRows((prev) => {
      const next = new Map(prev);
      const current = next.get(path);
      if (current) next.set(path, { ...current, ...patch });
      return next;
    });
  };

  const selectAllValid = () => {
    setTouched(true);
    setRows((prev) => {
      const next = new Map(prev);
      for (const candidate of candidates) {
        const current = next.get(candidate.path);
        if (current && candidate.valid && !doneRows.has(candidate.path)) {
          next.set(candidate.path, { ...current, checked: true });
        }
      }
      return next;
    });
  };

  const owner = scan?.source.repo.split("/")[0] ?? "";
  const renameTargets = useMemo(() => {
    const out = new Map<string, string>();
    for (const candidate of candidates) {
      const row = rows.get(candidate.path);
      if (row?.checked && row.resolution === "rename" && row.renameTo.trim()) {
        out.set(candidate.path, row.renameTo.trim().toLowerCase());
      }
    }
    return out;
  }, [candidates, rows]);

  const renameIsFree = (path: string, name: string): boolean => {
    const lower = name.trim().toLowerCase();
    if (!lower) return false;
    if (existingNames.some((existing) => existing.toLowerCase() === lower)) return false;
    for (const [otherPath, other] of renameTargets) {
      if (otherPath !== path && other === lower) return false;
    }
    // Not another candidate's own (new) name either.
    return !candidates.some(
      (c) => c.path !== path && c.valid && c.status === "new" && rows.get(c.path)?.checked && c.name.toLowerCase() === lower,
    );
  };

  const summary = useMemo(() => {
    const willImport: { candidate: SkillCandidate; as: string; action: "import" | "replace" | "rename" }[] = [];
    let unresolved = 0;
    let unchecked = 0;
    let files = 0;
    for (const candidate of candidates) {
      const row = rows.get(candidate.path);
      if (!row || !candidate.valid || doneRows.has(candidate.path)) continue;
      if (!row.checked) {
        // A same-commit duplicate is "already present", not "unchecked".
        if (candidate.status !== "same_commit") unchecked += 1;
        continue;
      }
      if (candidate.status === "name_taken") {
        if (!row.resolution) {
          unresolved += 1;
          continue;
        }
        if (row.resolution === "skip") continue;
        if (row.resolution === "rename") {
          if (!renameIsFree(candidate.path, row.renameTo)) {
            unresolved += 1;
            continue;
          }
          willImport.push({ candidate, as: row.renameTo.trim(), action: "rename" });
        } else {
          willImport.push({ candidate, as: candidate.name, action: "replace" });
        }
      } else {
        willImport.push({ candidate, as: candidate.name, action: "import" });
      }
      files += candidate.file_count;
    }
    const invalid = candidates.filter((c) => !c.valid).length;
    const sameCommit = candidates.filter((c) => c.status === "same_commit").length;
    return { willImport, unresolved, unchecked, invalid, sameCommit, files };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [candidates, rows, doneRows, existingNames, renameTargets]);

  const canImport =
    step === "results" && summary.willImport.length > 0 && summary.unresolved === 0 && folderName.trim() !== "";

  // ---- import -------------------------------------------------------------

  const submit = async () => {
    if (!canImport || !scan) return;
    setStep("importing");
    setImportError(null);
    const items: SkillImportItem[] = summary.willImport.map(({ candidate, as, action }) => ({
      path: candidate.path,
      action,
      ...(action === "rename" ? { name: as } : {}),
    }));
    try {
      const report = await importSkills({
        scan_id: scan.scan_id,
        source: text,
        folder: { name: folderName.trim(), parent_id: parentId },
        items,
      });
      const complete = report.failed.length === 0;
      if (complete) {
        await onImported(report, true);
        onClose();
        return;
      }
      const failed = new Map(report.failed.map((f) => [f.path, f.error]));
      const done = new Set(report.imported.map((r) => r.path));
      setFailedRows(failed);
      setDoneRows((prev) => new Set([...prev, ...done]));
      setRows((prev) => {
        const next = new Map(prev);
        for (const path of done) {
          const current = next.get(path);
          if (current) next.set(path, { ...current, checked: false });
        }
        return next;
      });
      setStep("results");
      await onImported(report, false);
    } catch (cause) {
      setImportError(cause instanceof Error ? cause.message : "Import failed");
      setStep("results");
    }
  };

  // ---- closing + keys -----------------------------------------------------

  const requestClose = useCallback(() => {
    if (step === "scanning") {
      cancelScan();
      return;
    }
    if (step === "results" && touched && !confirmDiscard) {
      setConfirmDiscard(true);
      return;
    }
    onClose();
  }, [step, touched, confirmDiscard, cancelScan, onClose]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (browsing) return; // the explorer owns Escape
      if (event.key === "Escape") {
        event.stopPropagation();
        requestClose();
        return;
      }
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "a" && step === "results") {
        const target = event.target as HTMLElement | null;
        if (target && (target.tagName === "INPUT" || target.tagName === "TEXTAREA")) return;
        event.preventDefault();
        selectAllValid();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [requestClose, step, browsing, candidates, doneRows]);

  // ---- rendering ----------------------------------------------------------

  const activeHowStep = step === "source" ? 0 : step === "scanning" ? 1 : 2;
  const showHowItWorks = step === "source" || step === "scanning";
  const filterNeedle = filter.trim().toLowerCase();
  const visible = candidates.filter(
    (c) =>
      !filterNeedle ||
      c.name.toLowerCase().includes(filterNeedle) ||
      c.description.toLowerCase().includes(filterNeedle) ||
      c.path.toLowerCase().includes(filterNeedle),
  );
  const validCount = candidates.filter((c) => c.valid).length;
  const inBankCount = candidates.filter((c) => c.status === "same_commit").length;
  const nothingFound = step === "results" && scan !== null && candidates.length === 0;
  const sourceLine = scan ? scan.source : null;

  return (
    <div
      className="fixed inset-0 z-[60] flex items-center justify-center bg-black/50"
      onClick={(event) => {
        event.stopPropagation();
        requestClose();
      }}
      data-testid="import-skills-backdrop"
    >
      <div
        className="flex w-[1020px] max-w-[96vw] max-h-[88vh] flex-col rounded-lg border border-line bg-bg-4 shadow-xl"
        onClick={(event) => event.stopPropagation()}
        role="dialog"
        aria-label="Import skills from a source"
        data-testid="import-skills-modal"
        data-step={step}
      >
        <div className="flex items-center justify-between border-b border-line px-4 py-3">
          <h3 className="flex items-center gap-2 font-semibold text-fg" style={{ fontSize: "13.5px" }}>
            <Download size={14} className="text-fg-3" />
            Import skills from a source
          </h3>
          <button
            type="button"
            onClick={requestClose}
            aria-label="Close import popup"
            className="grid h-6 w-6 place-items-center rounded text-fg-3 hover:bg-bg-5 hover:text-fg"
          >
            <X size={14} />
          </button>
        </div>

        <div className="flex min-h-0 flex-1 gap-4 p-4">
          {/* Left column */}
          <div className="flex min-w-0 flex-1 flex-col gap-3">
            {step === "results" || step === "importing" ? (
              <div
                className="flex items-center gap-2 rounded-md border border-acc/50 bg-bg-1 px-3 py-2"
                data-testid="import-source-line"
              >
                <GitBranch size={12} className="shrink-0 text-fg-4" />
                <span className="min-w-0 flex-1 truncate text-fg" style={{ fontSize: "11.5px" }}>
                  {text.trim()}
                </span>
                {scan?.commit && (
                  <span
                    className="rounded border border-line bg-bg-3 px-1.5 py-0.5 font-mono text-fg-3"
                    style={{ fontSize: "10px" }}
                    data-testid="import-commit"
                  >
                    @ {shortCommit(scan.commit)}
                  </span>
                )}
                <button
                  type="button"
                  onClick={changeSource}
                  disabled={step === "importing"}
                  data-testid="import-change-source"
                  className="rounded-md border border-line-strong bg-bg-3 px-2 py-0.5 text-fg-2 hover:border-acc disabled:opacity-40"
                  style={{ fontSize: "11px" }}
                >
                  Change
                </button>
              </div>
            ) : (
              <section>
                <h4 className="mb-2 text-fg-4 uppercase tracking-wide" style={{ fontSize: "10px" }}>
                  Source
                </h4>
                <div
                  className={`flex items-center gap-2 rounded-md border bg-bg-1 px-3 py-2 transition-colors ${
                    scanError ? "border-st-failed" : parsed ? "border-acc/60" : "border-line-strong focus-within:border-acc"
                  }`}
                >
                  <GitBranch size={12} className="shrink-0 text-fg-4" />
                  <input
                    ref={inputRef}
                    value={text}
                    onChange={(event) => {
                      setText(event.target.value);
                      setScanError(null);
                    }}
                    onKeyDown={(event) => {
                      if (event.key === "Enter") {
                        event.preventDefault();
                        startScan();
                      }
                    }}
                    disabled={step === "scanning"}
                    placeholder="https://github.com/owner/repo/tree/main/skills · git@… · /a/local/folder"
                    aria-label="Source"
                    aria-invalid={!!scanError || undefined}
                    data-testid="import-source-input"
                    spellCheck={false}
                    className="min-w-0 flex-1 bg-transparent text-fg outline-none placeholder:text-fg-4 disabled:opacity-60"
                    style={{ fontSize: "12px" }}
                  />
                  <button
                    type="button"
                    onClick={() => setBrowsing(true)}
                    disabled={step === "scanning"}
                    data-testid="import-browse-local"
                    className="flex shrink-0 items-center gap-1.5 rounded-md border border-line-strong bg-bg-3 px-2.5 py-1 text-fg-2 hover:border-acc disabled:opacity-40"
                    style={{ fontSize: "11px" }}
                  >
                    <HardDrive size={11} />
                    Browse local…
                  </button>
                </div>

                {parsed && step === "source" && !scanError && (
                  <div className="mt-2 flex flex-wrap items-center gap-1.5" data-testid="import-source-chips">
                    <Chip label={parsed.kind === "local" ? "folder" : "repo"} value={parsed.repo} />
                    {parsed.ref && <Chip label="ref" value={parsed.ref} />}
                    {parsed.path && <Chip label="path" value={parsed.path} />}
                    <Chip label="folder" value={parsed.suggestedFolder} accent />
                  </div>
                )}

                {scanError && (
                  <div
                    role="alert"
                    className="mt-2 rounded-md border border-st-failed/50 bg-st-failed-bg px-3 py-2 text-fg-2"
                    style={{ fontSize: "11px" }}
                    data-testid="import-scan-error"
                  >
                    <strong className="text-fg">
                      {scanError.code === "clone_failed" || scanError.code === "clone_timeout"
                        ? "Clone refused."
                        : scanError.code === "local_not_found"
                          ? "Folder not found."
                          : "Cannot scan."}
                    </strong>{" "}
                    <span className="font-mono">{scanError.message}</span>
                    {(scanError.code === "clone_failed" || scanError.code === "clone_timeout") && (
                      <>
                        {" "}
                        PDO uses the git credentials of the user running the daemon: log in with{" "}
                        <span className="font-mono">gh auth login</span>
                        {parsed && sshHint(parsed.url) && (
                          <>
                            {" "}
                            or use the SSH URL <span className="font-mono">{sshHint(parsed.url)}</span>
                          </>
                        )}
                        .
                      </>
                    )}{" "}
                    <button
                      type="button"
                      onClick={startScan}
                      data-testid="import-retry"
                      className="font-medium text-acc hover:underline"
                    >
                      Retry
                    </button>
                  </div>
                )}

                {step === "source" && !scanError && (
                  <p
                    className="mt-2 rounded-md border border-dashed border-line px-3 py-2 text-fg-4"
                    style={{ fontSize: "10.5px" }}
                  >
                    Accepted: a GitHub repo URL (root, branch, or{" "}
                    <span className="font-mono">/tree/&lt;branch&gt;/&lt;path&gt;</span>), an SSH URL, or a local
                    folder. PDO clones shallow with your git credentials and scans every folder holding a{" "}
                    <span className="font-mono">SKILL.md</span>. Nothing is written to the bank until you pick skills.
                  </p>
                )}
              </section>
            )}

            {step === "scanning" && (
              <div className="flex flex-1 flex-col items-center justify-center gap-3 py-10" data-testid="import-scanning">
                <div className="h-1 w-[360px] overflow-hidden rounded-full bg-bg-5">
                  <div className="h-full w-1/3 animate-pulse rounded-full bg-st-running" />
                </div>
                <div className="text-fg-2" style={{ fontSize: "12px" }}>
                  {parsed?.kind === "local" ? (
                    <>
                      Scanning <span className="font-mono">{relativise(parsed.url, home)}</span>…
                    </>
                  ) : (
                    <>
                      Cloning{" "}
                      <span className="font-mono">
                        {parsed?.repo}@{parsed?.ref ?? "default branch"}
                      </span>{" "}
                      (shallow, depth 1)…
                    </>
                  )}
                </div>
                <div className="text-fg-4" style={{ fontSize: "10.5px" }}>
                  Uses your git credentials · scanning starts when the clone lands
                </div>
                <button
                  type="button"
                  onClick={cancelScan}
                  data-testid="import-cancel-scan"
                  className="rounded-md border border-line-strong bg-bg-3 px-3 py-1.5 text-fg-2 hover:bg-bg-4"
                  style={{ fontSize: "11.5px" }}
                >
                  Cancel scan
                </button>
              </div>
            )}

            {step === "source" && recent.length > 0 && (
              <section>
                <h4 className="mb-2 text-fg-4 uppercase tracking-wide" style={{ fontSize: "10px" }}>
                  Recent sources
                </h4>
                <ul className="flex flex-col gap-1" data-testid="import-recent-sources">
                  {recent.map((item) => (
                    <li key={`${item.url}|${item.ref ?? ""}|${item.path}`}>
                      <button
                        type="button"
                        onClick={() => {
                          setText(recentToText(item));
                          setScanError(null);
                          inputRef.current?.focus();
                        }}
                        className="flex w-full items-center gap-2 rounded-md border border-line bg-bg-3 px-3 py-1.5 text-left hover:border-acc"
                        style={{ fontSize: "11.5px" }}
                      >
                        {/^https?:\/\/|^git@|^ssh:\/\/|^file:\/\//.test(item.url) ? (
                          <span className="h-2.5 w-2.5 shrink-0 rounded-full border-2 border-acc" />
                        ) : (
                          <Folder size={12} className="shrink-0 text-st-await" />
                        )}
                        <span className="font-mono text-fg">
                          {/^https?:\/\//.test(item.url) ? displaySourceUrl(item.url) : relativise(item.url, home)}
                          {item.path ? `/${item.path}` : ""}
                        </span>
                        <span className="text-fg-4">
                          {item.ref ? `· ${item.ref} ` : ""}· {timeAgo(item.last_used_at)}
                        </span>
                        <span className="flex-1" />
                        {item.folder_name && (
                          <span className="text-fg-4" style={{ fontSize: "10.5px" }}>
                            in bank as “{item.folder_name}”
                          </span>
                        )}
                      </button>
                    </li>
                  ))}
                </ul>
              </section>
            )}

            {nothingFound && sourceLine && (
              <div className="flex flex-1 flex-col items-center justify-center gap-2 py-10 text-center" data-testid="import-empty">
                <div className="text-fg" style={{ fontSize: "12.5px" }}>
                  No <span className="font-mono">SKILL.md</span>{" "}
                  {sourceLine.path ? (
                    <>
                      under <span className="font-mono">{sourceLine.path}/</span>.
                    </>
                  ) : (
                    "at this source."
                  )}
                </div>
                {scan && scan.elsewhere_count > 0 && (
                  <div className="text-fg-4" style={{ fontSize: "11px" }}>
                    Found {scan.elsewhere_count} in the repo elsewhere:{" "}
                    <button
                      type="button"
                      className="text-acc hover:underline"
                      data-testid="import-scan-whole-repo"
                      onClick={() => {
                        const next = sourceLine.ref ? `${sourceLine.url}/tree/${sourceLine.ref}` : sourceLine.url;
                        setText(next);
                        void runScan(next);
                      }}
                    >
                      scan the whole repo
                    </button>
                    {scan.elsewhere.slice(0, 3).map((dir) => (
                      <span key={dir}>
                        {" "}
                        or{" "}
                        <button
                          type="button"
                          className="font-mono text-acc hover:underline"
                          onClick={() => {
                            const next = `${sourceLine.url}/tree/${sourceLine.ref ?? "main"}/${dir}`;
                            setText(next);
                            void runScan(next);
                          }}
                        >
                          {dir || "."}/
                        </button>
                      </span>
                    ))}{" "}
                    instead.
                  </div>
                )}
              </div>
            )}

            {(step === "results" || step === "importing") && candidates.length > 0 && (
              <div className="flex min-h-0 flex-1 flex-col rounded-md border border-line bg-bg-1">
                <div className="flex items-center gap-2 border-b border-line px-3 py-2" style={{ fontSize: "11px" }}>
                  <span className="grid h-3.5 w-3.5 place-items-center rounded-sm bg-acc text-bg-1">
                    <Check size={9} strokeWidth={3} />
                  </span>
                  <span className="font-medium text-fg" data-testid="import-found-count">
                    {candidates.length} skill{candidates.length === 1 ? "" : "s"} found
                  </span>
                  <span className="text-fg-4">
                    · {validCount} valid · {summary.invalid} invalid
                    {inBankCount > 0 ? ` · ${inBankCount} already in bank` : ""}
                  </span>
                  <span className="flex-1" />
                  <label className="flex items-center gap-1.5 rounded-md border border-line-strong bg-bg-3 px-2 py-1">
                    <Search size={11} className="text-fg-4" />
                    <input
                      value={filter}
                      onChange={(event) => setFilter(event.target.value)}
                      placeholder="Filter…"
                      aria-label="Filter found skills"
                      data-testid="import-filter"
                      className="w-[120px] bg-transparent text-fg outline-none placeholder:text-fg-4"
                      style={{ fontSize: "11px" }}
                    />
                  </label>
                  <button
                    type="button"
                    onClick={selectAllValid}
                    data-testid="import-select-all"
                    className="text-acc hover:underline"
                  >
                    Select all valid
                  </button>
                </div>
                <ul className="min-h-0 flex-1 overflow-y-auto" data-testid="import-candidates">
                  {visible.map((candidate) => {
                    const row = rows.get(candidate.path) ?? defaultRow(candidate);
                    const failure = failedRows.get(candidate.path);
                    const done = doneRows.has(candidate.path);
                    const disabled = !candidate.valid || done || step === "importing";
                    const dim = candidate.status === "same_commit" || !candidate.valid || done;
                    return (
                      <li
                        key={candidate.path}
                        className={`border-b border-line px-3 py-2 last:border-b-0 ${failure ? "bg-st-failed-bg" : ""}`}
                        data-testid={`import-candidate-${candidate.name}`}
                        data-status={candidate.status}
                        data-checked={row.checked || undefined}
                      >
                        <div className="flex items-start gap-2.5">
                          <input
                            type="checkbox"
                            checked={row.checked && !done}
                            disabled={disabled}
                            onChange={(event) => setRow(candidate.path, { checked: event.target.checked })}
                            aria-label={`Import ${candidate.name}`}
                            className={`mt-0.5 h-3.5 w-3.5 shrink-0 accent-[var(--color-acc)] ${
                              !candidate.valid ? "opacity-40 [border-style:dashed]" : ""
                            }`}
                          />
                          <div className="min-w-0 flex-1">
                            <div className="flex items-center gap-2">
                              <span className={`font-semibold ${dim ? "text-fg-3" : "text-fg"}`} style={{ fontSize: "12.5px" }}>
                                {candidate.name}
                              </span>
                              {done && (
                                <span className="flex items-center gap-1 text-acc" style={{ fontSize: "10.5px" }}>
                                  <Check size={10} strokeWidth={3} /> imported
                                </span>
                              )}
                              <span className="flex-1" />
                              <StatusBadge candidate={candidate} />
                            </div>
                            {candidate.valid ? (
                              <div className={`mt-0.5 ${dim ? "text-fg-4" : "text-fg-3"}`} style={{ fontSize: "11px" }}>
                                {candidate.description}
                              </div>
                            ) : (
                              <div className="mt-0.5 text-st-failed" style={{ fontSize: "11px" }}>
                                Invalid frontmatter: {candidate.reason}
                              </div>
                            )}
                            <div className="mt-0.5 font-mono text-fg-4" style={{ fontSize: "10px" }}>
                              {candidate.path}/SKILL.md
                              {candidate.file_count > 0 && (
                                <>
                                  {" "}· {candidate.file_count} reference file{candidate.file_count === 1 ? "" : "s"}
                                </>
                              )}
                            </div>
                            {failure && (
                              <div className="mt-1 text-st-failed" style={{ fontSize: "11px" }} data-testid="import-row-error">
                                {failure}
                              </div>
                            )}
                            {candidate.valid && candidate.status === "name_taken" && row.checked && !done && (
                              <div className="mt-1.5 flex flex-wrap items-center gap-2" data-testid={`import-resolution-${candidate.name}`}>
                                <div className="flex overflow-hidden rounded-md border border-line-strong" role="radiogroup" aria-label={`Resolve ${candidate.name}`}>
                                  {(["replace", "rename", "skip"] as Resolution[]).map((option) => (
                                    <button
                                      key={option}
                                      type="button"
                                      role="radio"
                                      aria-checked={row.resolution === option}
                                      onClick={() =>
                                        setRow(candidate.path, {
                                          resolution: option,
                                          renameTo:
                                            option === "rename" && !row.renameTo
                                              ? `${candidate.name}${owner ? `-${owner}` : "-imported"}`
                                              : row.renameTo,
                                        })
                                      }
                                      className={`px-2.5 py-1 capitalize ${
                                        row.resolution === option ? "bg-bg-5 text-fg" : "text-fg-3 hover:bg-bg-3"
                                      }`}
                                      style={{ fontSize: "11px" }}
                                    >
                                      {option}
                                    </button>
                                  ))}
                                </div>
                                {row.resolution === "rename" && (
                                  <>
                                    <span className="text-fg-4" style={{ fontSize: "11px" }}>
                                      as
                                    </span>
                                    <input
                                      value={row.renameTo}
                                      onChange={(event) => setRow(candidate.path, { renameTo: event.target.value })}
                                      aria-label={`New name for ${candidate.name}`}
                                      data-testid={`import-rename-${candidate.name}`}
                                      spellCheck={false}
                                      className="w-60 rounded-md border border-line-strong bg-bg-1 px-2 py-0.5 font-mono text-fg outline-none focus:border-acc"
                                      style={{ fontSize: "11px" }}
                                    />
                                    {renameIsFree(candidate.path, row.renameTo) ? (
                                      <span className="flex items-center gap-1 text-acc" style={{ fontSize: "10.5px" }}>
                                        <Check size={10} strokeWidth={3} /> free
                                      </span>
                                    ) : (
                                      <span className="text-st-failed" style={{ fontSize: "10.5px" }}>
                                        {row.renameTo.trim() ? "taken" : "name required"}
                                      </span>
                                    )}
                                  </>
                                )}
                              </div>
                            )}
                          </div>
                        </div>
                      </li>
                    );
                  })}
                  {visible.length === 0 && (
                    <li className="px-3 py-6 text-center text-fg-4" style={{ fontSize: "11px" }}>
                      No skill matches “{filter.trim()}”.
                    </li>
                  )}
                </ul>
              </div>
            )}
          </div>

          {/* Right column */}
          <div className="flex w-[330px] shrink-0 flex-col gap-4 overflow-y-auto">
            {showHowItWorks && (
              <section>
                <h4 className="mb-2 text-fg-4 uppercase tracking-wide" style={{ fontSize: "10px" }}>
                  How it works
                </h4>
                <ol className="flex flex-col gap-1.5" data-testid="import-how-it-works">
                  {HOW_IT_WORKS.map((label, index) => (
                    <li
                      key={label}
                      className="flex items-center gap-2"
                      style={{ fontSize: "11.5px" }}
                      data-active={index === activeHowStep || undefined}
                    >
                      <span
                        className={`h-2 w-2 shrink-0 rounded-full ${
                          index < activeHowStep
                            ? "bg-acc"
                            : index === activeHowStep
                              ? step === "scanning"
                                ? "bg-st-running"
                                : "bg-acc"
                              : "border border-line-strong"
                        }`}
                      />
                      <span className={index <= activeHowStep ? "text-fg-2" : "text-fg-4"}>{label}</span>
                    </li>
                  ))}
                </ol>
              </section>
            )}

            {showHowItWorks && (
              <section>
                <h4 className="mb-2 text-fg-4 uppercase tracking-wide" style={{ fontSize: "10px" }}>
                  Into folder
                </h4>
                <ParentSelect folders={folders} value={parentId} onChange={setParentId} disabled={step === "scanning"} />
                <p className="mt-1.5 text-fg-4" style={{ fontSize: "10.5px" }}>
                  {parsed ? (
                    <>
                      A new folder <strong className="text-fg-2">{parsed.suggestedFolder}</strong> will be created here; you
                      can rename it after.
                    </>
                  ) : (
                    "A new folder named after the source will be created here."
                  )}
                </p>
              </section>
            )}

            {(step === "results" || step === "importing") && scan && (
              <>
                <section>
                  <h4 className="mb-2 text-fg-4 uppercase tracking-wide" style={{ fontSize: "10px" }}>
                    Destination
                  </h4>
                  <div className="flex items-center gap-2 rounded-md border border-line-strong bg-bg-3 px-2.5 py-1.5">
                    <FolderOpen size={12} className="shrink-0 text-st-await" />
                    {editingName ? (
                      <input
                        autoFocus
                        value={folderName}
                        onChange={(event) => setFolderName(event.target.value)}
                        onBlur={() => setEditingName(false)}
                        onKeyDown={(event) => {
                          if (event.key === "Enter" || event.key === "Escape") {
                            event.preventDefault();
                            event.stopPropagation();
                            setEditingName(false);
                          }
                        }}
                        aria-label="Folder name"
                        data-testid="import-folder-name-input"
                        className="min-w-0 flex-1 bg-transparent font-mono text-fg outline-none"
                        style={{ fontSize: "11.5px" }}
                      />
                    ) : (
                      <span className="min-w-0 flex-1 truncate font-mono text-fg" style={{ fontSize: "11.5px" }} data-testid="import-folder-name">
                        {folderName || <span className="text-st-failed">name required</span>}
                      </span>
                    )}
                    <button
                      type="button"
                      onClick={() => setEditingName(true)}
                      aria-label="Rename destination folder"
                      className="grid h-5 w-5 place-items-center rounded text-fg-4 hover:text-fg"
                    >
                      <Pencil size={11} />
                    </button>
                  </div>
                  <div className="mt-1.5">
                    <ParentSelect folders={folders} value={parentId} onChange={setParentId} disabled={step === "importing"} prefix="in" />
                  </div>
                  <p className="mt-1.5 text-fg-4" style={{ fontSize: "10.5px" }}>
                    New folder, tagged with this source and commit. Sub-paths are flattened: one skill = one row.
                  </p>
                </section>

                <section>
                  <h4 className="mb-2 text-fg-4 uppercase tracking-wide" style={{ fontSize: "10px" }}>
                    Will import
                  </h4>
                  <div className="overflow-hidden rounded-md border border-line" style={{ fontSize: "11px" }} data-testid="import-summary">
                    <SummaryRow label={`${summary.willImport.length} skill${summary.willImport.length === 1 ? "" : "s"}`}>
                      {summary.willImport.length === 0 ? (
                        <span className="text-fg-4">nothing checked</span>
                      ) : (
                        summary.willImport.map((item) => item.as).join(" · ")
                      )}
                    </SummaryRow>
                    {summary.willImport.some((item) => item.action === "rename") && (
                      <SummaryRow label={`${summary.willImport.filter((i) => i.action === "rename").length} renamed`}>
                        {summary.willImport
                          .filter((item) => item.action === "rename")
                          .map((item) => `${item.candidate.name} → ${item.as}`)
                          .join(" · ")}
                      </SummaryRow>
                    )}
                    {summary.willImport.some((item) => item.action === "replace") && (
                      <SummaryRow label={`${summary.willImport.filter((i) => i.action === "replace").length} replaced`}>
                        {summary.willImport
                          .filter((item) => item.action === "replace")
                          .map((item) => item.as)
                          .join(" · ")}
                      </SummaryRow>
                    )}
                    <SummaryRow label="files">
                      SKILL.md{summary.files > 0 ? ` + ${summary.files} reference file${summary.files === 1 ? "" : "s"}` : ""} copied
                      verbatim
                    </SummaryRow>
                  </div>
                </section>

                <section>
                  <h4 className="mb-2 text-fg-4 uppercase tracking-wide" style={{ fontSize: "10px" }}>
                    Not touched
                  </h4>
                  <p className="text-fg-4" style={{ fontSize: "10.5px" }} data-testid="import-not-touched">
                    {summary.unchecked} unchecked · {summary.invalid} invalid · {summary.sameCommit} already present at the
                    same commit.
                    {candidates
                      .filter((c) => c.status === "name_taken" && rows.get(c.path)?.resolution === "skip" && rows.get(c.path)?.checked)
                      .map((c) => (
                        <span key={c.path}>
                          {" "}
                          The existing “{c.existing?.name ?? c.name}”{c.existing?.folder_name ? ` in “${c.existing.folder_name}”` : ""} stays as is.
                        </span>
                      ))}
                  </p>
                </section>

                {summary.unresolved > 0 && (
                  <div
                    role="alert"
                    className="rounded-md border border-st-blocked/50 bg-st-blocked-bg px-3 py-2 text-fg-2"
                    style={{ fontSize: "11px" }}
                    data-testid="import-unresolved"
                  >
                    Resolve the name taken before importing: <strong className="text-fg">replace</strong> overwrites the content
                    of the bank's skill (its id and referents stay), <strong className="text-fg">rename</strong> keeps both,{" "}
                    <strong className="text-fg">skip</strong> leaves it out.
                  </div>
                )}

                {importError && (
                  <div role="alert" className="rounded-md border border-st-failed/50 bg-st-failed-bg px-3 py-2 text-fg-2" style={{ fontSize: "11px" }} data-testid="import-error">
                    {importError}
                  </div>
                )}
                {failedRows.size > 0 && !importError && (
                  <div role="alert" className="rounded-md border border-st-failed/50 bg-st-failed-bg px-3 py-2 text-fg-2" style={{ fontSize: "11px" }} data-testid="import-partial">
                    {failedRows.size} skill{failedRows.size === 1 ? "" : "s"} could not be imported; the reasons are on the rows. The
                    others landed in “{folderName}”.
                  </div>
                )}
              </>
            )}
          </div>
        </div>

        <div className="flex items-center justify-between gap-3 border-t border-line px-4 py-3">
          <span className="text-fg-4" style={{ fontSize: "10.5px" }}>
            {step === "scanning"
              ? "Esc cancels the scan"
              : step === "source"
                ? "Enter to scan · nothing touches disk until Import"
                : "Space toggles · ⌘A selects all valid · nothing touches disk until Import"}
          </span>
          <div className="flex items-center gap-2">
            {confirmDiscard ? (
              <>
                <span className="text-fg-2" style={{ fontSize: "11px" }} data-testid="import-discard-prompt">
                  Leave without importing?
                </span>
                <button
                  type="button"
                  onClick={() => setConfirmDiscard(false)}
                  className="rounded-md border border-line-strong bg-bg-3 px-3 py-1.5 text-fg-2 hover:bg-bg-4"
                  style={{ fontSize: "11.5px" }}
                >
                  Stay
                </button>
                <button
                  type="button"
                  onClick={onClose}
                  data-testid="import-discard"
                  className="rounded-md bg-st-failed px-3 py-1.5 font-medium text-white hover:opacity-90"
                  style={{ fontSize: "11.5px" }}
                >
                  Leave
                </button>
              </>
            ) : (
              <>
                <button
                  type="button"
                  onClick={requestClose}
                  data-testid="import-cancel"
                  className="rounded-md border border-line-strong bg-bg-3 px-3 py-1.5 text-fg-2 hover:bg-bg-4"
                  style={{ fontSize: "11.5px" }}
                >
                  Cancel
                </button>
                {step === "source" || step === "scanning" ? (
                  <button
                    type="button"
                    onClick={startScan}
                    disabled={!parsed || step === "scanning"}
                    data-testid="import-scan"
                    className="rounded-md bg-acc px-3 py-1.5 font-medium text-bg-1 transition-opacity hover:opacity-90 disabled:opacity-40"
                    style={{ fontSize: "11.5px" }}
                  >
                    {step === "scanning" ? "Scanning…" : "Scan source"}
                  </button>
                ) : (
                  <button
                    type="button"
                    onClick={() => void submit()}
                    disabled={!canImport}
                    data-testid="import-submit"
                    className="rounded-md bg-acc px-3 py-1.5 font-medium text-bg-1 transition-opacity hover:opacity-90 disabled:opacity-40"
                    style={{ fontSize: "11.5px" }}
                  >
                    {step === "importing"
                      ? "Importing…"
                      : `Import ${summary.willImport.length} skill${summary.willImport.length === 1 ? "" : "s"}`}
                  </button>
                )}
              </>
            )}
          </div>
        </div>
      </div>

      {browsing && (
        <FsExplorerModal
          mode="dir"
          title="Choose a local folder of skills"
          startPath={home ?? undefined}
          testIdPrefix="import-browse"
          onPick={(path) => {
            setText(path);
            setScanError(null);
          }}
          onClose={() => {
            setBrowsing(false);
            window.setTimeout(() => inputRef.current?.focus(), 0);
          }}
        />
      )}
    </div>
  );
}

function Chip({ label, value, accent }: { label: string; value: string; accent?: boolean }) {
  return (
    <span
      className={`flex items-center gap-1 rounded border px-1.5 py-0.5 ${accent ? "border-acc/40 bg-acc/10" : "border-line bg-bg-3"}`}
      style={{ fontSize: "10.5px" }}
    >
      <span className="text-fg-4">{label}</span>
      <span className="font-mono text-fg">{value}</span>
    </span>
  );
}

function StatusBadge({ candidate }: { candidate: SkillCandidate }) {
  const base = "rounded border px-1.5 py-0.5 whitespace-nowrap";
  const size = { fontSize: "9.5px" };
  switch (candidate.status) {
    case "new":
      return (
        <span className={`${base} border-acc/40 bg-acc/10 text-acc`} style={size}>
          new
        </span>
      );
    case "name_taken":
      return (
        <span className={`${base} border-st-blocked/50 bg-st-blocked-bg text-st-blocked`} style={size}>
          name taken{candidate.existing?.folder_name ? ` · in “${candidate.existing.folder_name}”` : ""}
        </span>
      );
    case "same_commit":
      return (
        <span className={`${base} border-line bg-bg-3 text-fg-4`} style={size}>
          same commit{candidate.existing?.folder_name ? ` · already in “${candidate.existing.folder_name}”` : " · already in bank"}
        </span>
      );
    default:
      return (
        <span className={`${base} border-st-failed/40 bg-st-failed-bg text-st-failed`} style={size}>
          not importable
        </span>
      );
  }
}

function SummaryRow({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex gap-3 border-t border-line px-3 py-1.5 first:border-t-0">
      <span className="w-[84px] shrink-0 text-fg-4">{label}</span>
      <span className="min-w-0 flex-1 text-fg-2">{children}</span>
    </div>
  );
}

function ParentSelect({
  folders,
  value,
  onChange,
  disabled,
  prefix,
}: {
  folders: SkillFolder[];
  value: string | null;
  onChange: (id: string | null) => void;
  disabled?: boolean;
  prefix?: string;
}) {
  return (
    <label className="flex items-center gap-2 rounded-md border border-line-strong bg-bg-3 px-2.5 py-1.5">
      {prefix ? (
        <span className="text-fg-4" style={{ fontSize: "10.5px" }}>
          {prefix}
        </span>
      ) : (
        <Folder size={12} className="shrink-0 text-st-await" />
      )}
      <select
        value={value ?? ""}
        onChange={(event) => onChange(event.target.value || null)}
        disabled={disabled}
        aria-label="Into folder"
        data-testid="import-parent-folder"
        className="w-full bg-transparent text-fg outline-none disabled:opacity-60"
        style={{ fontSize: "11.5px" }}
      >
        <option value="">Root of the bank</option>
        {folders.map((folder) => (
          <option key={folder.id} value={folder.id}>
            {folderPathLabel(folder.id, folders)}
          </option>
        ))}
      </select>
    </label>
  );
}
