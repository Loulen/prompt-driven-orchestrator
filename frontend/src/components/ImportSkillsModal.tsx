import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  ArrowRight,
  Check,
  Copy,
  Download,
  Equal,
  FileArchive,
  Folder,
  FolderOpen,
  GitBranch,
  HardDrive,
  Lock,
  Minus,
  Pencil,
  RefreshCw,
  Search,
  X,
} from "lucide-react";
import {
  ApiError,
  cancelSkillScan,
  fetchRecentSkillSources,
  importSkills,
  scanSkillArchive,
  scanSkillSource,
} from "../api";
import type {
  RecentSkillSource,
  SkillCandidate,
  SkillCandidateStatus,
  SkillFolder,
  SkillImportItem,
  SkillImportReport,
  SkillScanResult,
} from "../types";
import { ARCHIVE_FORMAT_LABEL, archiveDestination, refuseArchive } from "../lib/skillArchive";
import { useFileDropTarget } from "../hooks/useFileDropTarget";
import SkillFileDropZone, { DropOverlay } from "./SkillFileDropZone";
import { timeAgo } from "../lib/skillMd";
import { displaySourceUrl, parseSkillSource, shortCommit } from "../lib/skillSource";
import { folderPathLabel } from "../lib/skillTree";
import FsExplorerModal from "./FsExplorerModal";
import { tildify } from "../lib/homePath";

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
   * failure: the popup stays open with the failed rows in red. `reported` is
   * true for an archive (#1028): the popup stays open on its per-line report,
   * which replaces the host's toast.
   */
  onImported: (report: SkillImportReport, complete: boolean, reported?: boolean) => void | Promise<void>;
  /** « Show “<folder>” in the bank » from the report (#1028); closes the popup. */
  onShowFolder?: (folderId: string) => void;
}

type Step = "source" | "scanning" | "results" | "importing" | "report";

type Resolution = "replace" | "rename" | "skip";

/** The four answers to a row whose id the bank already has, content changed (#1028). */
type IdChoice = "keep" | "replace" | "move" | "duplicate";
const ID_CHOICES: { value: IdChoice; label: string }[] = [
  { value: "keep", label: "Keep as is" },
  { value: "replace", label: "Replace" },
  { value: "move", label: "Move" },
  { value: "duplicate", label: "Duplicate" },
];

interface RowState {
  checked: boolean;
  resolution: Resolution | null;
  renameTo: string;
  /** `id_changed` rows only (#1028); « Keep as is » by default. */
  idChoice: IdChoice;
  /** The copy's name for « Duplicate ». */
  duplicateAs: string;
}

/** What a write row of the import will do. */
type WriteAction = "import" | "replace" | "rename" | "replace_id" | "move" | "duplicate";

interface ScanError {
  code: string;
  message: string;
}

const HOW_IT_WORKS = [
  "Point at a source, or drop a .zip",
  "Scan finds every SKILL.md",
  "Check the skills to import",
  "They land in a folder named after the source",
];

function newScanId(): string {
  const c = globalThis.crypto as Crypto | undefined;
  if (c && typeof c.randomUUID === "function") return c.randomUUID();
  return `scan-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
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

/** A checked row whose name is already in the bank must say replace / rename / skip. */
function needsResolution(candidate: SkillCandidate): boolean {
  return candidate.status === "name_taken" || candidate.status === "same_commit";
}

/**
 * Statuses of an archive row whose **id** the bank already has (#1027): left
 * as is, greyed and never checkable — « already up to date » or « managed by
 * PDO ». A « changed » row is checkable and offers keep / replace / move /
 * duplicate (#1028).
 */
const KEPT_BY_ID: ReadonlySet<SkillCandidateStatus> = new Set(["id_up_to_date", "locked"]);

/** Can the row be checked at all? */
function isSelectable(candidate: SkillCandidate): boolean {
  return candidate.valid && !KEPT_BY_ID.has(candidate.status);
}

const ARCHIVE_HINT = "A .zip only — a PDO skills archive, a pipeline's skills sidecar, or any zip of SKILL.md folders";

/** The in-place refusal of a dropped file (never a modal over this modal). */
interface ArchiveRefusal {
  title: string;
  message: string;
  /** Add the « drop a PDO archive… » advice (the daemon's `no_skills` already says it). */
  advise: boolean;
}
function defaultRow(candidate: SkillCandidate): RowState {
  return {
    checked: candidate.status === "new" || candidate.status === "name_taken" || candidate.status === "id_changed",
    resolution: null,
    renameTo: "",
    idChoice: "keep",
    duplicateAs: "",
  };
}

/** `<name>-2`, `-3`… : the first label free among `taken` (lower-cased). */
function freeSuffixedName(name: string, taken: Set<string>): string {
  for (let n = 2; n < 10_000; n += 1) {
    const next = `${name}-${n}`;
    if (!taken.has(next.toLowerCase())) return next;
  }
  return `${name}-copy`;
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
  onShowFolder,
}: Props) {
  const [step, setStep] = useState<Step>("source");
  const [text, setText] = useState("");
  const [parentId, setParentId] = useState<string | null>(initialFolderId);
  const [folderName, setFolderName] = useState("");
  /** Set when the user chose to import into an existing folder of that name. */
  const [intoFolderId, setIntoFolderId] = useState<string | null>(null);
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
  /** The `.zip` being read or listed (#1027); `null` for a typed source. */
  const [archiveFile, setArchiveFile] = useState<File | null>(null);
  const [archiveRefusal, setArchiveRefusal] = useState<ArchiveRefusal | null>(null);
  /** Destination of an archive whose folder already exists: merge or new (default). */
  const [destChoice, setDestChoice] = useState<"new" | "merge">("new");
  const [dropName, setDropName] = useState<string | null>(null);
  /** The import's outcome, shown line by line for an archive (#1028). */
  const [report, setReport] = useState<SkillImportReport | null>(null);
  /** The rows as they were sent, for the report's « kept » / « unchecked » lines. */
  const [sentRows, setSentRows] = useState<Map<string, RowState>>(new Map());
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
    async (source: string | File) => {
      const id = newScanId();
      scanIdRef.current = id;
      const isFile = typeof source !== "string";
      setStep("scanning");
      setScanError(null);
      setArchiveRefusal(null);
      setArchiveFile(isFile ? source : null);
      setScan(null);
      setFailedRows(new Map());
      setDoneRows(new Set());
      setImportError(null);
      setTouched(false);
      try {
        const result = isFile ? await scanSkillArchive(id, source, source.name) : await scanSkillSource(id, source);
        if (scanIdRef.current !== id) return; // cancelled or superseded
        setScan(result);
        setRows(new Map(result.candidates.map((c) => [c.path, defaultRow(c)])));
        if (result.archive) {
          // Created skills land in a folder named after the file; when it exists
          // the default is a new, suffixed one (the user may merge instead).
          const dest =
            parentId === null
              ? { newName: result.archive.new_folder_name }
              : archiveDestination(result.source.suggested_folder, folders, parentId);
          setFolderName(dest.newName);
          setDestChoice("new");
        } else {
          setFolderName(result.source.suggested_folder);
        }
        setIntoFolderId(null);
        setStep("results");
      } catch (cause) {
        if (scanIdRef.current !== id) return;
        const code =
          cause instanceof ApiError && typeof (cause.body as { code?: unknown } | null)?.code === "string"
            ? ((cause.body as { code: string }).code)
            : "";
        const message = cause instanceof Error ? cause.message : "Scan failed";
        if (isFile) {
          setArchiveFile(null);
          setArchiveRefusal({
            title: archiveRefusalTitle(code),
            message: asSentence(message),
            advise: code === "not_a_zip" || code === "corrupt_archive" || code === "invalid_upload",
          });
        } else {
          setScanError({ code, message });
        }
        setStep("source");
      }
    },
    [folders, parentId],
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
    setArchiveFile(null);
    setStep("source");
    setTouched(false);
    window.setTimeout(() => inputRef.current?.focus(), 0);
  };

  /** A `.zip` dropped anywhere on the popup or picked with Browse… (#1027). */
  const acceptArchive = (files: ArrayLike<File>) => {
    if (step === "scanning" || step === "importing" || files.length === 0) return;
    const refusal = refuseArchive(files);
    if (refusal) {
      setScan(null);
      setArchiveFile(null);
      setStep("source");
      setArchiveRefusal({ title: "Not a skills archive.", message: refusal, advise: true });
      return;
    }
    void runScan(files[0]);
  };

  const { dragging, handlers: dropHandlers } = useFileDropTarget(
    (dataTransfer) => acceptArchive(dataTransfer.files),
    (dataTransfer) => setDropName(dataTransfer.files?.[0]?.name ?? null),
    step === "source" || step === "results",
  );

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
        if (current && isSelectable(candidate) && !doneRows.has(candidate.path)) {
          next.set(candidate.path, { ...current, checked: true });
        }
      }
      return next;
    });
  };

  // #771: the bulk counterpart of "Select all valid" — until it existed a user who
  // wanted 2 skills out of 40 pre-checked ones had to uncheck 38 boxes one by one.
  const deselectAll = () => {
    setTouched(true);
    setRows((prev) => {
      const next = new Map(prev);
      for (const candidate of candidates) {
        const current = next.get(candidate.path);
        if (current && !doneRows.has(candidate.path)) {
          next.set(candidate.path, { ...current, checked: false });
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
      if (row?.checked && candidate.status === "id_changed" && row.idChoice === "duplicate" && row.duplicateAs.trim()) {
        out.set(candidate.path, row.duplicateAs.trim().toLowerCase());
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
    const willImport: { candidate: SkillCandidate; as: string; action: WriteAction }[] = [];
    let unresolved = 0;
    let unresolvedDuplicates = 0;
    let unchecked = 0;
    let kept = 0;
    let files = 0;
    for (const candidate of candidates) {
      const row = rows.get(candidate.path);
      if (!row || !isSelectable(candidate) || doneRows.has(candidate.path)) continue;
      if (!row.checked) {
        // A same-commit duplicate is "already present", not "unchecked".
        if (candidate.status !== "same_commit") unchecked += 1;
        continue;
      }
      if (candidate.status === "id_changed") {
        // #1028: the bank has this id with another content.
        if (row.idChoice === "keep") {
          kept += 1;
          continue;
        }
        if (row.idChoice === "duplicate") {
          if (!renameIsFree(candidate.path, row.duplicateAs)) {
            unresolvedDuplicates += 1;
            continue;
          }
          willImport.push({ candidate, as: row.duplicateAs.trim(), action: "duplicate" });
        } else {
          willImport.push({ candidate, as: candidate.name, action: row.idChoice === "move" ? "move" : "replace_id" });
        }
        files += candidate.file_count;
        continue;
      }
      if (needsResolution(candidate)) {
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
    const invalid = candidates.filter((c) => !c.valid && c.status !== "locked").length;
    const sameCommit = candidates.filter((c) => c.status === "same_commit").length;
    const upToDate = candidates.filter((c) => c.status === "id_up_to_date").length;
    const locked = candidates.filter((c) => c.status === "locked").length;
    return { willImport, unresolved, unresolvedDuplicates, unchecked, kept, invalid, sameCommit, upToDate, locked, files };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [candidates, rows, doneRows, existingNames, renameTargets]);

  const isArchive = scan?.source.kind === "file";
  /**
   * An archive's destination (#1027): the folder already named after the file
   * under the chosen parent — the daemon reports it at the root — offered for
   * merge, else a new folder.
   */
  const archiveExisting = useMemo((): { id: string; name: string } | null => {
    if (!scan?.archive) return null;
    if (parentId === null) return scan.archive.existing_folder;
    return archiveDestination(scan.source.suggested_folder, folders, parentId).existing;
  }, [scan, folders, parentId]);
  const mergeInto = isArchive && destChoice === "merge" ? archiveExisting : null;

  const intoFolder = intoFolderId ? folders.find((f) => f.id === intoFolderId) ?? null : null;
  /** A sibling folder already carrying the destination name: warn, offer to reuse it. */
  const homonym = useMemo(() => {
    const needle = folderName.trim().toLowerCase();
    if (!needle || intoFolderId || isArchive) return null;
    return folders.find((f) => f.name.toLowerCase() === needle && (f.parent_id ?? null) === parentId) ?? null;
  }, [folders, folderName, parentId, intoFolderId, isArchive]);

  const canImport =
    step === "results" &&
    summary.willImport.length > 0 &&
    summary.unresolved === 0 &&
    summary.unresolvedDuplicates === 0 &&
    (intoFolder !== null || mergeInto !== null || folderName.trim() !== "");
  const destinationName = mergeInto?.name ?? intoFolder?.name ?? folderName;
  /** The existing folder the import writes into, if any (merge / reuse). */
  const destinationId = mergeInto?.id ?? intoFolder?.id ?? null;

  // ---- import -------------------------------------------------------------

  const submit = async () => {
    if (!canImport || !scan) return;
    setStep("importing");
    setImportError(null);
    const items: SkillImportItem[] = summary.willImport.map(({ candidate, as, action }) => ({
      path: candidate.path,
      action: action === "replace_id" ? "replace" : action,
      ...(action === "rename" || action === "duplicate" ? { name: as } : {}),
    }));
    if (isArchive) {
      // #1028: a known id is left as is unless the row says otherwise — say so.
      for (const candidate of candidates) {
        const row = rows.get(candidate.path);
        const keepChanged = candidate.status === "id_changed" && row?.checked && row.idChoice === "keep";
        if (keepChanged || candidate.status === "id_up_to_date") items.push({ path: candidate.path, action: "keep" });
      }
    }
    try {
      const folder = mergeInto
        ? { id: mergeInto.id }
        : intoFolder
          ? { id: intoFolder.id }
          : { name: folderName.trim(), parent_id: parentId };
      const report = await importSkills(
        isArchive
          ? { scan_id: scan.scan_id, source: scan.source.url, kind: "file", folder, items }
          : { scan_id: scan.scan_id, source: text, folder, items },
      );
      const complete = report.failed.length === 0;
      if (isArchive) {
        // #1028: an archive ends on its per-line report, not on a toast.
        setSentRows(new Map(rows));
        setReport(report);
        setStep("report");
        await onImported(report, complete, true);
        return;
      }
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
    const wrote = doneRows.size > 0 || failedRows.size > 0 || step === "report";
    if (step === "results" && touched && !confirmDiscard && !wrote) {
      setConfirmDiscard(true);
      return;
    }
    onClose();
  }, [step, touched, confirmDiscard, doneRows, failedRows, cancelScan, onClose]);

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
        className="relative flex w-[1020px] max-w-[96vw] max-h-[88vh] flex-col rounded-lg border border-line bg-bg-4 shadow-xl"
        onClick={(event) => event.stopPropagation()}
        role="dialog"
        aria-label="Import skills from a source"
        data-testid="import-skills-modal"
        data-step={step}
        {...dropHandlers}
      >
        {/* #1027: the whole popup accepts a dropped .zip. */}
        {dragging !== null && (
          <DropOverlay count={dragging} title={`Drop to import ${dropName ?? "the .zip"}`} hint={ARCHIVE_HINT} />
        )}
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
            {step === "results" || step === "importing" || step === "report" ? (
              <div
                className="flex items-center gap-2 rounded-md border border-acc/50 bg-bg-1 px-3 py-2"
                data-testid="import-source-line"
              >
                {isArchive && scan ? (
                  <>
                    <FileArchive size={12} className="shrink-0 text-fg-4" />
                    <span className="min-w-0 flex-1 truncate font-mono text-fg" style={{ fontSize: "11.5px" }} data-testid="import-archive-name">
                      {scan.source.url}
                    </span>
                    {scan.archive && (
                      <span
                        className="whitespace-nowrap rounded border border-line bg-bg-3 px-1.5 py-0.5 text-fg-3"
                        style={{ fontSize: "10px" }}
                        data-testid="import-archive-format"
                        data-format={scan.archive.format}
                      >
                        {ARCHIVE_FORMAT_LABEL[scan.archive.format]}
                      </span>
                    )}
                  </>
                ) : (
                  <>
                    <GitBranch size={12} className="shrink-0 text-fg-4" />
                    <span className="min-w-0 flex-1 truncate text-fg" style={{ fontSize: "11.5px" }}>
                      {text.trim()}
                    </span>
                  </>
                )}
                {scan?.commit && (
                  <span
                    className="rounded border border-line bg-bg-3 px-1.5 py-0.5 font-mono text-fg-3"
                    style={{ fontSize: "10px" }}
                    data-testid="import-commit"
                  >
                    @ {shortCommit(scan.commit)}
                  </span>
                )}
                {step !== "report" && (
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
                )}
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

                <div className="my-2 flex items-center gap-2 text-fg-4" style={{ fontSize: "10.5px" }} aria-hidden>
                  <span className="h-px flex-1 bg-line" />
                  or an archive
                  <span
                    className="rounded-sm bg-fuchsia-600 px-1 font-medium text-white"
                    style={{ fontSize: "9px", lineHeight: "13px" }}
                    data-testid="import-archive-new"
                  >
                    new
                  </span>
                  <span className="h-px flex-1 bg-line" />
                </div>
                <SkillFileDropZone
                  label={
                    <>
                      <span className="font-mono text-fg-2">.zip</span> · drop it anywhere in this popup, or
                    </>
                  }
                  onPickFiles={acceptArchive}
                  accept=".zip,application/zip"
                  multiple={false}
                  ariaLabel="Import a .zip archive"
                  testId="import-archive-drop"
                  disabled={step === "scanning"}
                />

                {archiveRefusal && step === "source" && (
                  <div
                    role="alert"
                    className="mt-2 rounded-md border border-st-failed/50 bg-st-failed-bg px-3 py-2 text-fg-2"
                    style={{ fontSize: "11px" }}
                    data-testid="import-archive-error"
                  >
                    <strong className="text-fg">{archiveRefusal.title}</strong> {archiveRefusal.message}
                    {archiveRefusal.advise && (
                      <>
                        {" "}
                        Drop a PDO archive, a pipeline's skills sidecar, or a zip of folders holding a{" "}
                        <span className="font-mono">SKILL.md</span>.
                      </>
                    )}
                  </div>
                )}

                {step === "source" && !scanError && !archiveRefusal && (
                  <p
                    className="mt-2 rounded-md border border-dashed border-line px-3 py-2 text-fg-4"
                    style={{ fontSize: "10.5px" }}
                    data-testid="import-accepted"
                  >
                    Accepted: a GitHub repo URL (root, branch, or{" "}
                    <span className="font-mono">/tree/&lt;branch&gt;/&lt;path&gt;</span>), an SSH URL, a local folder — or
                    a <span className="font-mono">.zip</span>: an archive exported from a PDO bank (ids kept), a
                    pipeline's skills sidecar, or any zip of <span className="font-mono">SKILL.md</span> folders (matched by
                    name). PDO clones shallow with your git credentials and scans every folder holding a{" "}
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
                  {archiveFile ? (
                    <>
                      Reading <span className="font-mono">{archiveFile.name}</span>…
                    </>
                  ) : parsed?.kind === "local" ? (
                    <>
                      Scanning <span className="font-mono">{tildify(parsed.url, home)}</span>…
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
                  {archiveFile
                    ? "Uploaded to the daemon and unpacked in a temp folder · nothing touches the bank"
                    : "Uses your git credentials · scanning starts when the clone lands"}
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
                          {/^https?:\/\//.test(item.url) ? displaySourceUrl(item.url) : tildify(item.url, home)}
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

            {step === "report" && report && (
              <ImportReportList report={report} candidates={candidates} rows={sentRows} folders={folders} />
            )}

            {(step === "results" || step === "importing") && candidates.length > 0 && (
              <div className="flex min-h-0 flex-1 flex-col rounded-md border border-line bg-bg-1">
                <div className="flex items-center gap-2 border-b border-line px-3 py-2" style={{ fontSize: "11px" }}>
                  {summary.willImport.length > 0 ? (
                    <span
                      className="grid h-3.5 w-3.5 place-items-center rounded-sm bg-acc text-bg-1"
                      data-testid="import-found-check"
                    >
                      <Check size={9} strokeWidth={3} />
                    </span>
                  ) : (
                    <span className="h-3.5 w-3.5 rounded-sm border border-line-strong" aria-hidden />
                  )}
                  <span className="font-medium text-fg" data-testid="import-found-count">
                    {candidates.length} skill{candidates.length === 1 ? "" : "s"} found
                  </span>
                  <span className="text-fg-4" data-testid="import-found-breakdown">
                    {isArchive
                      ? archiveBreakdown(candidates)
                      : `· ${validCount} valid · ${summary.invalid} invalid${inBankCount > 0 ? ` · ${inBankCount} already in bank` : ""}`}
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
                  <span className="text-fg-5">·</span>
                  <button
                    type="button"
                    onClick={deselectAll}
                    data-testid="import-deselect-all"
                    className="text-acc hover:underline"
                  >
                    Deselect all
                  </button>
                </div>
                <ul className="min-h-0 flex-1 overflow-y-auto" data-testid="import-candidates">
                  {visible.map((candidate) => {
                    const row = rows.get(candidate.path) ?? defaultRow(candidate);
                    const failure = failedRows.get(candidate.path);
                    const done = doneRows.has(candidate.path);
                    const keptById = KEPT_BY_ID.has(candidate.status);
                    const disabled = !isSelectable(candidate) || done || step === "importing";
                    const dim = candidate.status === "same_commit" || !candidate.valid || keptById || done;
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
                            checked={row.checked && !done && !keptById}
                            disabled={disabled}
                            onChange={(event) => setRow(candidate.path, { checked: event.target.checked })}
                            aria-label={`Import ${candidate.name}`}
                            className={`mt-0.5 h-3.5 w-3.5 shrink-0 accent-[var(--color-acc)] ${
                              !isSelectable(candidate) ? "opacity-40 [border-style:dashed]" : ""
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
                            ) : candidate.status === "locked" ? null : (
                              <div className="mt-0.5 text-st-failed" style={{ fontSize: "11px" }}>
                                {candidate.code === "file_too_large" ? candidate.reason : `Invalid frontmatter: ${candidate.reason}`}
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
                            {candidate.status === "id_changed" && row.checked && !done && (
                              <IdChoiceControl
                                candidate={candidate}
                                row={row}
                                disabled={step === "importing"}
                                destinationName={destinationName}
                                destinationId={destinationId}
                                nameTaken={(name) => !renameIsFree(candidate.path, name)}
                                onChoose={(choice) =>
                                  setRow(candidate.path, {
                                    idChoice: choice,
                                    duplicateAs:
                                      choice === "duplicate" && !row.duplicateAs
                                        ? freeSuffixedName(
                                            candidate.name,
                                            new Set([
                                              ...existingNames.map((n) => n.toLowerCase()),
                                              ...[...renameTargets.entries()]
                                                .filter(([p]) => p !== candidate.path)
                                                .map(([, n]) => n),
                                            ]),
                                          )
                                        : row.duplicateAs,
                                  })
                                }
                                onRename={(name) => setRow(candidate.path, { duplicateAs: name })}
                              />
                            )}
                            {failure && (
                              <div className="mt-1 text-st-failed" style={{ fontSize: "11px" }} data-testid="import-row-error">
                                {failure}
                              </div>
                            )}
                            {candidate.valid && needsResolution(candidate) && row.checked && !done && (
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
                                              ? `${candidate.name}${owner && !isArchive ? `-${owner}` : "-imported"}`
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

            {step === "report" && report && scan && (
              <>
                <section data-testid="import-report-destination">
                  <h4 className="mb-2 text-fg-4 uppercase tracking-wide" style={{ fontSize: "10px" }}>
                    Destination
                  </h4>
                  <div className="flex items-center gap-2 rounded-md border border-line-strong bg-bg-3 px-2.5 py-1.5">
                    <FolderOpen size={12} className="shrink-0 text-st-await" />
                    <span className="min-w-0 flex-1 truncate font-mono text-fg" style={{ fontSize: "11.5px" }}>
                      {report.folder.name}
                    </span>
                    <span className="rounded border border-line bg-bg-1 px-1.5 py-0.5 text-fg-4" style={{ fontSize: "9.5px" }}>
                      {destinationId ? "existing" : "new"}
                    </span>
                  </div>
                  <p className="mt-1.5 text-fg-4" style={{ fontSize: "10.5px" }}>
                    {destinationId
                      ? `The skills it created are tagged “imported from ${scan.source.url}”; the folder keeps its own provenance.`
                      : `Tagged “imported from ${scan.source.url}” — a file has no update: re-import a newer archive instead.`}
                  </p>
                </section>
                <section>
                  <h4 className="mb-2 text-fg-4 uppercase tracking-wide" style={{ fontSize: "10px" }}>
                    Next
                  </h4>
                  <p className="text-fg-4" style={{ fontSize: "10.5px" }} data-testid="import-report-next">
                    Pipelines referencing these ids find them now. Replaced skills kept their id, so their referents are
                    untouched.
                  </p>
                </section>
              </>
            )}

            {(step === "results" || step === "importing") && scan && (
              <>
                {isArchive ? (
                  <section data-testid="import-archive-destination">
                    <h4 className="mb-2 text-fg-4 uppercase tracking-wide" style={{ fontSize: "10px" }}>
                      Destination
                    </h4>
                    {archiveExisting ? (
                      <>
                        <p className="mb-2 text-fg-3" style={{ fontSize: "10.5px" }}>
                          A folder <strong className="text-fg">{archiveExisting.name}</strong> already exists{" "}
                          {parentId === null ? "at the root of the bank" : `in “${folderPathLabel(parentId, folders)}”`}.
                        </p>
                        <div className="flex flex-col gap-2" role="radiogroup" aria-label="Destination folder">
                          <DestinationOption
                            selected={destChoice === "new"}
                            onSelect={() => setDestChoice("new")}
                            disabled={step === "importing"}
                            title="New folder"
                            note="Default. Nothing in the existing folder moves."
                            testId="import-dest-new"
                          >
                            {destChoice === "new" && (
                              <div className="mt-2">
                                <NewFolderNameField
                                  value={folderName}
                                  onChange={setFolderName}
                                  editing={editingName}
                                  onEditingChange={setEditingName}
                                />
                              </div>
                            )}
                          </DestinationOption>
                          <DestinationOption
                            selected={destChoice === "merge"}
                            onSelect={() => setDestChoice("merge")}
                            disabled={step === "importing"}
                            title={`Merge into “${archiveExisting.name}”`}
                            note="New skills join the existing folder. It keeps its own provenance."
                            testId="import-dest-merge"
                          />
                        </div>
                      </>
                    ) : (
                      <NewFolderNameField
                        value={folderName}
                        onChange={setFolderName}
                        editing={editingName}
                        onEditingChange={setEditingName}
                      />
                    )}
                    {!mergeInto &&
                      folders.some(
                        (f) =>
                          (f.parent_id ?? null) === parentId &&
                          f.name.trim().toLowerCase() === folderName.trim().toLowerCase(),
                      ) && (
                        <div
                          role="alert"
                          className="mt-1.5 rounded-md border border-st-blocked/50 bg-st-blocked-bg px-2.5 py-1.5 text-fg-2"
                          style={{ fontSize: "10.5px" }}
                          data-testid="import-archive-homonym"
                        >
                          A folder named “{folderName.trim()}” already exists here: importing creates a second one next to
                          it.{archiveExisting ? " Merge into it instead, or pick another name." : " Pick another name."}
                        </div>
                      )}
                    <p className="mt-1.5 text-fg-4" style={{ fontSize: "10.5px" }} data-testid="import-archive-note">
                      {mergeInto
                        ? `The skills it creates are tagged “imported from ${scan.source.url}”; the folder keeps its own provenance.`
                        : `Tagged “imported from ${scan.source.url}” — a file has no update: re-import a newer archive instead.`}
                    </p>
                  </section>
                ) : (
                <section>
                  <h4 className="mb-2 text-fg-4 uppercase tracking-wide" style={{ fontSize: "10px" }}>
                    Destination
                  </h4>
                  {intoFolder ? (
                    <div className="flex items-center gap-2 rounded-md border border-line-strong bg-bg-3 px-2.5 py-1.5" data-testid="import-into-existing">
                      <FolderOpen size={12} className="shrink-0 text-st-await" />
                      <span className="min-w-0 flex-1 truncate font-mono text-fg" style={{ fontSize: "11.5px" }} data-testid="import-folder-name">
                        {folderPathLabel(intoFolder.id, folders)}
                      </span>
                      <span className="rounded border border-line bg-bg-1 px-1.5 py-0.5 text-fg-4" style={{ fontSize: "9.5px" }}>
                        existing
                      </span>
                      <button
                        type="button"
                        onClick={() => setIntoFolderId(null)}
                        data-testid="import-into-new"
                        className="text-acc hover:underline"
                        style={{ fontSize: "10.5px" }}
                      >
                        New folder instead
                      </button>
                    </div>
                  ) : (
                  <NewFolderNameField
                    value={folderName}
                    onChange={setFolderName}
                    editing={editingName}
                    onEditingChange={setEditingName}
                  />
                  )}
                  {!intoFolder && (
                    <div className="mt-1.5">
                      <ParentSelect folders={folders} value={parentId} onChange={setParentId} disabled={step === "importing"} prefix="in" />
                    </div>
                  )}
                  {homonym ? (
                    <div
                      role="alert"
                      className="mt-1.5 rounded-md border border-st-blocked/50 bg-st-blocked-bg px-2.5 py-1.5 text-fg-2"
                      style={{ fontSize: "10.5px" }}
                      data-testid="import-folder-homonym"
                    >
                      A folder named “{homonym.name}” already exists here
                      {homonym.source ? " (imported from a source)" : ""}. Importing would create a second one next to it.{" "}
                      <button
                        type="button"
                        onClick={() => setIntoFolderId(homonym.id)}
                        data-testid="import-use-existing"
                        className="text-acc hover:underline"
                      >
                        Import into the existing folder
                      </button>{" "}
                      or rename the destination.
                    </div>
                  ) : (
                    <p className="mt-1.5 text-fg-4" style={{ fontSize: "10.5px" }}>
                      {intoFolder
                        ? "Existing folder: it takes this source and commit as provenance. Sub-paths are flattened: one skill = one row."
                        : "New folder, tagged with this source and commit. Sub-paths are flattened: one skill = one row."}
                    </p>
                  )}
                </section>
                )}

                <section>
                  <h4 className="mb-2 text-fg-4 uppercase tracking-wide" style={{ fontSize: "10px" }}>
                    Will import
                  </h4>
                  <div className="overflow-hidden rounded-md border border-line" style={{ fontSize: "11px" }} data-testid="import-summary">
                    {isArchive ? (
                      <ArchiveSummaryRows willImport={summary.willImport} />
                    ) : (
                    <>
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
                    </>
                    )}
                  </div>
                </section>

                <section>
                  <h4 className="mb-2 text-fg-4 uppercase tracking-wide" style={{ fontSize: "10px" }}>
                    Not touched
                  </h4>
                  <p className="text-fg-4" style={{ fontSize: "10.5px" }} data-testid="import-not-touched">
                    {isArchive
                      ? `${summary.kept} kept as is · ${summary.upToDate} already up to date · ${summary.unchecked} unchecked · ${summary.locked} managed by PDO · ${summary.invalid} invalid.`
                      : `${summary.unchecked} unchecked · ${summary.invalid} invalid · ${summary.sameCommit} already present at the same commit.`}
                    {candidates
                      .filter((c) => needsResolution(c) && rows.get(c.path)?.resolution === "skip" && rows.get(c.path)?.checked)
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
                    Resolve the name already in the bank before importing: <strong className="text-fg">replace</strong> overwrites the content
                    of the bank's skill (its id and referents stay), <strong className="text-fg">rename</strong> keeps both,{" "}
                    <strong className="text-fg">skip</strong> leaves it out.
                  </div>
                )}

                {summary.unresolvedDuplicates > 0 && (
                  <div
                    role="alert"
                    className="rounded-md border border-st-blocked/50 bg-st-blocked-bg px-3 py-2 text-fg-2"
                    style={{ fontSize: "11px" }}
                    data-testid="import-duplicate-unresolved"
                  >
                    Give each <strong className="text-fg">duplicate</strong> a name that is free in the bank before importing.
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
                    others landed in “{destinationName}”.
                  </div>
                )}
              </>
            )}
          </div>
        </div>

        <div className="flex items-center justify-between gap-3 border-t border-line px-4 py-3">
          <span className="text-fg-4" style={{ fontSize: "10.5px" }}>
            {step === "report"
              ? "Esc closes · the report stays until you close it"
              : step === "scanning"
              ? "Esc cancels the scan"
              : step === "source"
                ? "Enter to scan · drop a .zip to read it · nothing touches disk until Import"
                : "Space toggles · ⌘A selects all valid · nothing touches disk until Import"}
          </span>
          <div className="flex items-center gap-2">
            {step === "report" && report ? (
              <>
                <button
                  type="button"
                  onClick={onClose}
                  data-testid="import-report-close"
                  className="rounded-md border border-line-strong bg-bg-3 px-3 py-1.5 text-fg-2 hover:bg-bg-4"
                  style={{ fontSize: "11.5px" }}
                >
                  Close
                </button>
                {report.imported.length > 0 && (
                  <button
                    type="button"
                    onClick={() => {
                      onShowFolder?.(report.folder.id);
                      onClose();
                    }}
                    data-testid="import-report-show-folder"
                    className="rounded-md bg-acc px-3 py-1.5 font-medium text-bg-1 transition-opacity hover:opacity-90"
                    style={{ fontSize: "11.5px" }}
                  >
                    Show “{report.folder.name}” in the bank
                  </button>
                )}
              </>
            ) : confirmDiscard ? (
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

/** The refusal's bold lead, one per named daemon reason (#1027). */
function archiveRefusalTitle(code: string): string {
  switch (code) {
    case "archive_too_large":
      return "Archive too large.";
    case "corrupt_archive":
      return "Corrupted archive.";
    case "unsafe_entry":
      return "Unsafe archive.";
    case "no_skills":
      return "No skills in this archive.";
    default:
      return "Not a skills archive.";
  }
}

/** A message shown after a bold lead reads as a sentence: it ends with a period. */
function asSentence(message: string): string {
  const trimmed = message.trim();
  return /[.!?]$/.test(trimmed) ? trimmed : `${trimmed}.`;
}

/** `· 1 new · 2 changed · 1 up to date · 1 name taken · 2 not importable` (#1027). */
function archiveBreakdown(candidates: SkillCandidate[]): string {
  const count = (pred: (c: SkillCandidate) => boolean) => candidates.filter(pred).length;
  const parts: [number, string][] = [
    [count((c) => c.status === "new"), "new"],
    [count((c) => c.status === "id_changed"), "changed"],
    [count((c) => c.status === "id_up_to_date"), "up to date"],
    [count((c) => c.status === "name_taken" || c.status === "same_commit"), "name taken"],
    [count((c) => c.status === "invalid" || c.status === "locked"), "not importable"],
  ];
  const shown = parts.filter(([n]) => n > 0).map(([n, label]) => `${n} ${label}`);
  return shown.length > 0 ? `· ${shown.join(" · ")}` : "";
}

function NewFolderNameField({
  value,
  onChange,
  editing,
  onEditingChange,
}: {
  value: string;
  onChange: (value: string) => void;
  editing: boolean;
  onEditingChange: (editing: boolean) => void;
}) {
  return (
    <div className="flex items-center gap-2 rounded-md border border-line-strong bg-bg-3 px-2.5 py-1.5">
      <FolderOpen size={12} className="shrink-0 text-st-await" />
      {editing ? (
        <input
          autoFocus
          value={value}
          onChange={(event) => onChange(event.target.value)}
          onBlur={() => onEditingChange(false)}
          onKeyDown={(event) => {
            if (event.key === "Enter" || event.key === "Escape") {
              event.preventDefault();
              event.stopPropagation();
              onEditingChange(false);
            }
          }}
          aria-label="Folder name"
          data-testid="import-folder-name-input"
          className="min-w-0 flex-1 bg-transparent font-mono text-fg outline-none"
          style={{ fontSize: "11.5px" }}
        />
      ) : (
        <span className="min-w-0 flex-1 truncate font-mono text-fg" style={{ fontSize: "11.5px" }} data-testid="import-folder-name">
          {value || <span className="text-st-failed">name required</span>}
        </span>
      )}
      <button
        type="button"
        onClick={() => onEditingChange(true)}
        aria-label="Rename destination folder"
        className="grid h-5 w-5 place-items-center rounded text-fg-4 hover:text-fg"
      >
        <Pencil size={11} />
      </button>
    </div>
  );
}

/** One card of the merge / new-folder radio group of an archive import (#1027). */
function DestinationOption({
  selected,
  onSelect,
  disabled,
  title,
  note,
  testId,
  children,
}: {
  selected: boolean;
  onSelect: () => void;
  disabled?: boolean;
  title: string;
  note: string;
  testId: string;
  children?: React.ReactNode;
}) {
  return (
    <div
      className={`rounded-md border px-3 py-2 ${selected ? "border-acc/60 bg-acc/5" : "border-line bg-bg-3"}`}
      data-testid={testId}
      data-selected={selected || undefined}
    >
      <button
        type="button"
        role="radio"
        aria-checked={selected}
        disabled={disabled}
        onClick={onSelect}
        className="flex w-full items-start gap-2 text-left disabled:opacity-60"
      >
        <span
          className={`mt-0.5 grid h-3 w-3 shrink-0 place-items-center rounded-full border ${selected ? "border-acc" : "border-line-strong"}`}
        >
          {selected && <span className="h-1.5 w-1.5 rounded-full bg-acc" />}
        </span>
        <span className="min-w-0 flex-1">
          <span className="block text-fg" style={{ fontSize: "11.5px" }}>
            {title}
          </span>
          <span className="block text-fg-4" style={{ fontSize: "10.5px" }}>
            {note}
          </span>
        </span>
      </button>
      {children}
    </div>
  );
}

function StatusBadge({ candidate }: { candidate: SkillCandidate }) {
  const base = "rounded border px-1.5 py-0.5 whitespace-nowrap";
  const size = { fontSize: "9.5px" };
  const inFolder = candidate.existing?.folder_name ? ` · in “${candidate.existing.folder_name}”` : "";
  switch (candidate.status) {
    case "new":
      return (
        <span className={`${base} border-acc/40 bg-acc/10 text-acc`} style={size}>
          {candidate.id ? "new · id kept" : "new"}
        </span>
      );
    case "id_up_to_date":
      return (
        <span className={`${base} border-line bg-bg-3 text-fg-4`} style={size}>
          already up to date{inFolder}
        </span>
      );
    case "id_changed":
      return (
        <span className={`${base} border-st-await/50 bg-st-await/10 text-st-await`} style={size}>
          changed{inFolder}
        </span>
      );
    case "locked":
      return (
        <span className={`${base} flex items-center gap-1 border-line bg-bg-3 text-fg-4`} style={size}>
          <Lock size={9} aria-hidden /> managed by PDO
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

/** `in “ops”`, or `at the root of the bank` for a skill outside any folder. */
function whereLabel(folderName: string | null | undefined): string {
  return folderName ? `in “${folderName}”` : "at the root of the bank";
}

/**
 * The four-way choice of a row whose id the bank already has with another
 * content (#1028, ADR-0082): Keep as is (default) · Replace · Move · Duplicate,
 * with a line saying what the choice does here, given the destination.
 */
function IdChoiceControl({
  candidate,
  row,
  disabled,
  destinationName,
  destinationId,
  nameTaken,
  onChoose,
  onRename,
}: {
  candidate: SkillCandidate;
  row: RowState;
  disabled: boolean;
  destinationName: string;
  destinationId: string | null;
  nameTaken: (name: string) => boolean;
  onChoose: (choice: IdChoice) => void;
  onRename: (name: string) => void;
}) {
  const existing = candidate.existing;
  const where = whereLabel(existing?.folder_name);
  const dest = `“${destinationName.trim() || "the new folder"}”`;
  const renamed = existing && existing.name.toLowerCase() !== candidate.name.toLowerCase();
  const nameNote = !renamed
    ? ""
    : nameTaken(candidate.name)
      ? ` The name “${candidate.name}” is taken in the bank: it keeps “${existing.name}”.`
      : ` It is renamed “${candidate.name}”.`;
  const sameFolder = destinationId !== null && destinationId === (existing?.folder_id ?? null);
  let explanation: string;
  switch (row.idChoice) {
    case "replace":
      explanation = `The bank's copy ${where} takes the archive's content and files; it keeps its id and stays ${where}.${nameNote}`;
      break;
    case "move":
      explanation = sameFolder
        ? `Same as Replace here: the skill already lives in ${dest}.${nameNote}`
        : `Replaced, then moved from ${existing?.folder_name ? `“${existing.folder_name}”` : "the root"} to ${dest}; its id is kept.${nameNote}`;
      break;
    case "duplicate":
      explanation = `A copy under a new id lands in ${dest}; the bank's copy ${where} stays untouched — pipelines keep referencing it.`;
      break;
    default:
      explanation = `The bank's copy ${where} stays untouched; the archive's version is ignored.`;
  }
  const dupTrimmed = row.duplicateAs.trim();
  return (
    <div className="mt-1.5" data-testid={`import-id-choice-${candidate.name}`} data-choice={row.idChoice}>
      <div className="flex flex-wrap items-center gap-2">
        <div className="flex overflow-hidden rounded-md border border-line-strong" role="radiogroup" aria-label={`Known skill ${candidate.name}`}>
          {ID_CHOICES.map((option) => (
            <button
              key={option.value}
              type="button"
              role="radio"
              aria-checked={row.idChoice === option.value}
              disabled={disabled}
              onClick={() => onChoose(option.value)}
              className={`px-2.5 py-1 ${row.idChoice === option.value ? "bg-bg-5 text-fg" : "text-fg-3 hover:bg-bg-3"}`}
              style={{ fontSize: "11px" }}
            >
              {option.label}
            </button>
          ))}
        </div>
        {row.idChoice === "duplicate" && (
          <>
            <span className="text-fg-4" style={{ fontSize: "11px" }}>
              as
            </span>
            <input
              value={row.duplicateAs}
              onChange={(event) => onRename(event.target.value)}
              disabled={disabled}
              aria-label={`Name of the copy of ${candidate.name}`}
              data-testid={`import-duplicate-${candidate.name}`}
              spellCheck={false}
              className="w-52 rounded-md border border-line-strong bg-bg-1 px-2 py-0.5 font-mono text-fg outline-none focus:border-acc"
              style={{ fontSize: "11px" }}
            />
            {dupTrimmed && !nameTaken(dupTrimmed) ? (
              <span className="flex items-center gap-1 text-acc" style={{ fontSize: "10.5px" }} data-testid={`import-duplicate-free-${candidate.name}`}>
                <Check size={10} strokeWidth={3} /> free
              </span>
            ) : (
              <span className="text-st-failed" style={{ fontSize: "10.5px" }} data-testid={`import-duplicate-taken-${candidate.name}`}>
                {dupTrimmed ? "taken" : "name required"}
              </span>
            )}
          </>
        )}
      </div>
      <div className="mt-1 text-fg-4" style={{ fontSize: "10.5px" }} data-testid={`import-id-explain-${candidate.name}`}>
        {explanation}
      </div>
    </div>
  );
}

/** « Will import » of an archive (#1028): new / replaced / moved / duplicated / renamed. */
function ArchiveSummaryRows({
  willImport,
}: {
  willImport: { candidate: SkillCandidate; as: string; action: WriteAction }[];
}) {
  const groups: { label: string; items: string[] }[] = [
    { label: "new", items: willImport.filter((i) => i.action === "import").map((i) => i.as) },
    {
      label: "replaced",
      items: willImport.filter((i) => i.action === "replace" || i.action === "replace_id").map((i) => i.as),
    },
    { label: "moved", items: willImport.filter((i) => i.action === "move").map((i) => i.as) },
    {
      label: "duplicated",
      items: willImport.filter((i) => i.action === "duplicate").map((i) => `${i.candidate.name} → ${i.as}`),
    },
    {
      label: "renamed",
      items: willImport.filter((i) => i.action === "rename").map((i) => `${i.candidate.name} → ${i.as}`),
    },
  ].filter((group) => group.items.length > 0);
  if (groups.length === 0) {
    return (
      <SummaryRow label="0 skills">
        <span className="text-fg-4">nothing would be written</span>
      </SummaryRow>
    );
  }
  return (
    <>
      {groups.map((group) => (
        <SummaryRow key={group.label} label={`${group.items.length} ${group.label}`}>
          {group.items.join(" · ")}
        </SummaryRow>
      ))}
    </>
  );
}

interface ReportLine {
  path: string;
  name: string;
  icon: React.ReactNode;
  text: string;
  tone: "ok" | "dim" | "failed";
  warning?: string;
}

/** One line per skill of the archive, after the import (#1028). */
function reportLines(
  report: SkillImportReport,
  candidates: SkillCandidate[],
  rows: Map<string, RowState>,
  folders: SkillFolder[],
): ReportLine[] {
  const folderName = (id: string | null | undefined) =>
    id ? folders.find((f) => f.id === id)?.name ?? (id === report.folder.id ? report.folder.name : null) : null;
  const quote = (name: string | null) => (name ? `“${name}”` : "the root");
  const into = `“${report.folder.name}”`;
  return candidates.map((candidate): ReportLine => {
    const base = { path: candidate.path, name: candidate.name };
    const failed = report.failed.find((f) => f.path === candidate.path);
    if (failed) return { ...base, icon: <X size={12} />, text: `failed: ${failed.error}`, tone: "failed" };
    const done = report.imported.find((r) => r.path === candidate.path);
    if (done) {
      const warning = done.warning;
      switch (done.action) {
        case "replaced":
          return {
            ...base,
            icon: <RefreshCw size={12} />,
            text: `replaced, stays ${whereLabel(folderName(done.skill.folder_id))}`,
            tone: "ok",
            warning,
          };
        case "moved": {
          const from = folderName(done.previous_folder_id) ?? candidate.existing?.folder_name ?? null;
          return {
            ...base,
            icon: <ArrowRight size={12} />,
            text: `replaced, moved ${quote(from)} → ${into}`,
            tone: "ok",
            warning,
          };
        }
        case "duplicated":
          return {
            ...base,
            icon: <Copy size={12} />,
            text: `duplicated as “${done.skill.name}” · new id, in ${into}`,
            tone: "ok",
            warning,
          };
        case "renamed":
          return { ...base, icon: <Check size={12} />, text: `imported as “${done.skill.name}”`, tone: "ok", warning };
        default:
          return {
            ...base,
            icon: <Check size={12} />,
            text: candidate.id && done.skill.id === candidate.id ? "imported, id kept" : "imported",
            tone: "ok",
            warning,
          };
      }
    }
    const where = whereLabel(candidate.existing?.folder_name);
    const row = rows.get(candidate.path);
    switch (candidate.status) {
      case "id_up_to_date":
        return { ...base, icon: <Equal size={12} />, text: `already up to date ${where}`, tone: "dim" };
      case "locked":
        return { ...base, icon: <Lock size={12} />, text: "managed by PDO here — not imported", tone: "dim" };
      case "invalid":
        return {
          ...base,
          icon: <X size={12} />,
          text: candidate.code === "file_too_large" ? `invalid: ${candidate.reason}` : `invalid: ${candidate.reason ?? "not a valid SKILL.md"}`,
          tone: "failed",
        };
      case "id_changed":
        if (row?.checked) return { ...base, icon: <Equal size={12} />, text: `kept as is ${where}`, tone: "dim" };
        break;
      case "same_commit":
        return { ...base, icon: <Equal size={12} />, text: "already present at the same commit", tone: "dim" };
      default:
        if (row?.checked && row.resolution === "skip") {
          return { ...base, icon: <Minus size={12} />, text: `skipped — the existing “${candidate.existing?.name ?? candidate.name}” stays as is`, tone: "dim" };
        }
    }
    return { ...base, icon: <Minus size={12} />, text: "unchecked — not imported", tone: "dim" };
  });
}

function ImportReportList({
  report,
  candidates,
  rows,
  folders,
}: {
  report: SkillImportReport;
  candidates: SkillCandidate[];
  rows: Map<string, RowState>;
  folders: SkillFolder[];
}) {
  const lines = reportLines(report, candidates, rows, folders);
  const written = report.imported.length;
  const failed = report.failed.length;
  return (
    <div className="flex min-h-0 flex-1 flex-col rounded-md border border-line bg-bg-1" data-testid="import-report">
      <div className="flex items-center gap-2 border-b border-line px-3 py-2" style={{ fontSize: "11px" }}>
        <span
          className={`grid h-3.5 w-3.5 place-items-center rounded-sm ${failed > 0 ? "bg-st-failed" : "bg-acc"} text-bg-1`}
          aria-hidden
        >
          {failed > 0 ? <X size={9} strokeWidth={3} /> : <Check size={9} strokeWidth={3} />}
        </span>
        <span className="font-medium text-fg" data-testid="import-report-title">
          Import done · {written} written{failed > 0 ? ` · ${failed} failed` : ""}
        </span>
        <span className="text-fg-4">into “{report.folder.name}” · one line per skill of the archive</span>
      </div>
      <ul className="min-h-0 flex-1 overflow-y-auto">
        {lines.map((line) => (
          <li
            key={line.path}
            className="flex items-start gap-3 border-b border-line px-3 py-2 last:border-b-0"
            data-testid={`import-report-${line.name}`}
            data-tone={line.tone}
          >
            <span className={`mt-0.5 shrink-0 ${line.tone === "ok" ? "text-acc" : line.tone === "failed" ? "text-st-failed" : "text-fg-4"}`}>
              {line.icon}
            </span>
            <span className={`w-[160px] shrink-0 truncate font-mono ${line.tone === "dim" ? "text-fg-3" : "text-fg"}`} style={{ fontSize: "11.5px" }}>
              {line.name}
            </span>
            <span className="min-w-0 flex-1" style={{ fontSize: "11px" }}>
              <span className={line.tone === "failed" ? "text-st-failed" : "text-fg-2"}>{line.text}</span>
              {line.warning && (
                <span className="block text-st-blocked" style={{ fontSize: "10.5px" }} data-testid={`import-report-warning-${line.name}`}>
                  {line.warning}
                </span>
              )}
            </span>
          </li>
        ))}
      </ul>
    </div>
  );
}
