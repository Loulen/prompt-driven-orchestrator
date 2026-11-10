import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ApiError } from "../../shared/http/client";
import { cancelSkillScan, fetchRecentSkillSources, importSkills, scanSkillSource } from "../api/import";
import type { RecentSkillSource } from "../../shared/generated/RecentSkillSource";
import type { SkillFolder } from "../../shared/generated/SkillFolder";
import type { SkillImportItem } from "../../shared/generated/SkillImportItem";
import type { SkillImportReport } from "../../shared/generated/SkillImportReport";
import type { SkillScanResult } from "../model/skill";
import { parseSkillSource } from "../model/skillSource";
import { newScanId } from "../model/scanId";
import {
  defaultRow,
  importSummary,
  isRenameFree,
  renameTargetsOf,
  type RowState,
  type ScanError,
  type Step,
} from "../model/importRows";

/**
 * The state of "Import skills from a source" (#670): the typed source, the
 * scan (cancellable, superseded by a newer one), the checkable rows with their
 * resolutions, the destination folder, and the import. Nothing touches the
 * bank before `submit`.
 */
export function useImportSkills({
  folders,
  existingNames,
  initialFolderId,
  onClose,
  onImported,
}: {
  folders: SkillFolder[];
  existingNames: string[];
  initialFolderId: string | null;
  onClose: () => void;
  onImported: (report: SkillImportReport, complete: boolean) => void | Promise<void>;
}) {
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
        setIntoFolderId(null);
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
  const renameTargets = useMemo(() => renameTargetsOf(candidates, rows), [candidates, rows]);
  const renameIsFree = (path: string, name: string): boolean =>
    isRenameFree(path, name, existingNames, renameTargets, candidates, rows);

  const summary = useMemo(
    () => importSummary(candidates, rows, doneRows, renameIsFree),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [candidates, rows, doneRows, existingNames, renameTargets],
  );

  const intoFolder = intoFolderId ? folders.find((f) => f.id === intoFolderId) ?? null : null;
  /** A sibling folder already carrying the destination name: warn, offer to reuse it. */
  const homonym = useMemo(() => {
    const needle = folderName.trim().toLowerCase();
    if (!needle || intoFolderId) return null;
    return folders.find((f) => f.name.toLowerCase() === needle && (f.parent_id ?? null) === parentId) ?? null;
  }, [folders, folderName, parentId, intoFolderId]);

  const canImport =
    step === "results" &&
    summary.willImport.length > 0 &&
    summary.unresolved === 0 &&
    (intoFolder !== null || folderName.trim() !== "");

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
        folder: intoFolder ? { id: intoFolder.id } : { name: folderName.trim(), parent_id: parentId },
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
    const wrote = doneRows.size > 0 || failedRows.size > 0;
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

  return {
    step, text, setText, parentId, setParentId, folderName, setFolderName, intoFolderId, setIntoFolderId,
    editingName, setEditingName, recent, browsing, setBrowsing, scan, scanError, setScanError, rows,
    filter, setFilter, failedRows, doneRows, importError, confirmDiscard, setConfirmDiscard, inputRef,
    parsed, runScan, cancelScan, startScan, changeSource, candidates, setRow, selectAllValid, deselectAll,
    owner, renameIsFree, summary, intoFolder, homonym, canImport, submit, requestClose,
  };
}

export type ImportSkillsState = ReturnType<typeof useImportSkills>;
