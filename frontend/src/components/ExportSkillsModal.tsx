import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { FileArchive, Lock, RotateCcw, Search, Upload, X } from "lucide-react";
import { ApiError, exportSkillArchive, fetchSkill, type SkillArchiveDownload } from "../api";
import type { Skill, SkillFolder } from "../types";
import type { TreeNodeRef, TreeRow } from "../lib/skillTree";
import {
  defaultArchiveName,
  exportableInFolder,
  isExportable,
  withZip,
} from "../lib/skillArchive";
import SkillTree from "./SkillTree";

interface Props {
  folders: SkillFolder[];
  skills: Skill[];
  /** Skill ids checked when the modal opens (seeded ones are ignored). */
  initialChecked?: string[];
  onClose: () => void;
  /** The archive is downloaded; the parent closes the modal and says so. */
  onExported: (download: SkillArchiveDownload) => void;
  /** Injectable for tests: hands the zip to the browser. */
  save?: (blob: Blob, fileName: string) => void;
  /** Injectable for tests: the clock behind `pdo-skills-<YYYYMMDD-HHMM>.zip`. */
  now?: () => Date;
}

function saveBlob(blob: Blob, fileName: string) {
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = fileName;
  anchor.click();
  URL.revokeObjectURL(url);
}

const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;

/**
 * « Export skills to an archive » (pdo-issues#2, #1026, ADR-0084) — the shell of
 * the import modal, mirrored. Left: the bank's tree with checkboxes (a folder's
 * box checks its skills *now*, a snapshot; seeded skills are greyed « Managed by
 * PDO »). Right: the editable file name (it follows the selection until the user
 * types), what will and will not be exported, and how to import on the other
 * side. The bank is never modified; the download is the browser's.
 */
export default function ExportSkillsModal({
  folders,
  skills,
  initialChecked = [],
  onClose,
  onExported,
  save = saveBlob,
  now = () => new Date(),
}: Props) {
  const skillById = useMemo(() => new Map(skills.map((skill) => [skill.id, skill])), [skills]);
  const exportable = useMemo(() => skills.filter(isExportable), [skills]);
  const seeded = useMemo(() => skills.filter((skill) => !isExportable(skill)), [skills]);
  const [checked, setChecked] = useState<Set<string>>(
    () => new Set(initialChecked.filter((id) => { const s = skillById.get(id); return !!s && isExportable(s); })),
  );
  const [expanded, setExpanded] = useState<Set<string>>(() => new Set(folders.map((folder) => folder.id)));
  const [filter, setFilter] = useState("");
  const [selected, setSelected] = useState<TreeNodeRef | null>(null);
  /** `null` = the name follows the selection; a string = the user typed it. */
  const [typedName, setTypedName] = useState<string | null>(null);
  const [exporting, setExporting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [fileCounts, setFileCounts] = useState<Map<string, number>>(() => new Map());
  const pointerRef = useRef(false);
  /** The stamp of `pdo-skills-<YYYYMMDD-HHMM>.zip`: when the modal opened. */
  const [openedAt] = useState(now);

  const defaultName = useMemo(
    () => defaultArchiveName(checked, folders, skills, openedAt),
    [checked, folders, skills, openedAt],
  );
  const nameInput = typedName ?? defaultName;
  const resolvedName = withZip(nameInput);
  const nameMissing = resolvedName === "";
  const appended = nameInput.trim() !== "" && resolvedName !== nameInput.trim();

  // Reference-file counts for « Will export », read lazily per checked skill.
  useEffect(() => {
    const missing = [...checked].filter((id) => !fileCounts.has(id));
    if (missing.length === 0) return;
    let cancelled = false;
    void Promise.all(
      missing.map((id) =>
        fetchSkill(id)
          .then((detail) => [id, detail.files.length] as const)
          .catch(() => [id, -1] as const),
      ),
    ).then((pairs) => {
      if (cancelled) return;
      setFileCounts((prev) => {
        const next = new Map(prev);
        for (const [id, n] of pairs) next.set(id, n);
        return next;
      });
    });
    return () => {
      cancelled = true;
    };
  }, [checked, fileCounts]);

  const checkedSkills = useMemo(
    () =>
      [...checked]
        .map((id) => skillById.get(id))
        .filter((skill): skill is Skill => !!skill)
        .sort((a, b) => a.name.localeCompare(b.name, undefined, { sensitivity: "base" })),
    [checked, skillById],
  );
  const knownFiles = checkedSkills.every((skill) => (fileCounts.get(skill.id) ?? -1) >= 0);
  const referenceFiles = checkedSkills.reduce((sum, skill) => sum + Math.max(0, fileCounts.get(skill.id) ?? 0), 0);

  const toggleSkill = useCallback(
    (id: string) => {
      const skill = skillById.get(id);
      if (!skill || !isExportable(skill) || exporting) return;
      setChecked((prev) => {
        const next = new Set(prev);
        if (next.has(id)) next.delete(id);
        else next.add(id);
        return next;
      });
    },
    [skillById, exporting],
  );
  const toggleFolder = useCallback(
    (folderId: string) => {
      if (exporting) return;
      const ids = exportableInFolder(folderId, folders, skills);
      if (ids.length === 0) return;
      setChecked((prev) => {
        const next = new Set(prev);
        if (ids.every((id) => next.has(id))) ids.forEach((id) => next.delete(id));
        else ids.forEach((id) => next.add(id));
        return next;
      });
    },
    [folders, skills, exporting],
  );
  const toggleRef = useCallback(
    (ref: TreeNodeRef) => (ref.kind === "folder" ? toggleFolder(ref.id) : toggleSkill(ref.id)),
    [toggleFolder, toggleSkill],
  );
  const selectAll = useCallback(() => {
    if (exporting) return;
    const needle = filter.trim().toLowerCase();
    setChecked((prev) => {
      const next = new Set(prev);
      for (const skill of exportable) {
        if (
          !needle ||
          skill.name.toLowerCase().includes(needle) ||
          skill.description.toLowerCase().includes(needle)
        ) {
          next.add(skill.id);
        }
      }
      return next;
    });
  }, [exportable, filter, exporting]);
  const deselectAll = () => {
    if (!exporting) setChecked(new Set());
  };

  const canExport = checked.size > 0 && !nameMissing && !exporting;
  const submit = useCallback(async () => {
    if (!canExport) return;
    setExporting(true);
    setError(null);
    try {
      const ids = checkedSkills.map((skill) => skill.id);
      const download = await exportSkillArchive(ids, resolvedName);
      save(download.blob, download.fileName);
      onExported(download);
    } catch (cause) {
      setError(cause instanceof ApiError || cause instanceof Error ? cause.message : "Export failed");
      setExporting(false);
    }
  }, [canExport, checkedSkills, resolvedName, save, onExported]);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      const inField = !!target && (target.tagName === "INPUT" || target.tagName === "TEXTAREA");
      if (event.key === "Escape") {
        event.stopPropagation();
        if (!exporting) onClose();
        return;
      }
      if ((event.metaKey || event.ctrlKey) && event.key === "Enter") {
        event.preventDefault();
        void submit();
        return;
      }
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "a" && !inField) {
        event.preventDefault();
        selectAll();
        return;
      }
      if (event.key === " " && !inField && selected) {
        event.preventDefault();
        toggleRef(selected);
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [onClose, submit, selectAll, selected, toggleRef, exporting]);

  // ---- rendering ----------------------------------------------------------

  const renderLeading = (row: TreeRow) => {
    if (row.ref.kind === "folder" && row.folder) {
      const ids = exportableInFolder(row.folder.id, folders, skills);
      const all = ids.length > 0 && ids.every((id) => checked.has(id));
      const some = !all && ids.some((id) => checked.has(id));
      return (
        <input
          type="checkbox"
          aria-label={`Export every skill of ${row.folder.name}`}
          data-testid={`export-folder-check-${row.folder.id}`}
          checked={all}
          ref={(el) => {
            if (el) el.indeterminate = some;
          }}
          disabled={ids.length === 0 || exporting}
          onClick={(event) => event.stopPropagation()}
          onChange={() => toggleFolder(row.folder!.id)}
          className="h-3.5 w-3.5 shrink-0 accent-[var(--color-acc)] disabled:opacity-40"
        />
      );
    }
    if (row.skill) {
      const locked = !isExportable(row.skill);
      return (
        <input
          type="checkbox"
          aria-label={`Export ${row.skill.name}`}
          data-testid={`export-check-${row.skill.id}`}
          checked={!locked && checked.has(row.skill.id)}
          disabled={locked || exporting}
          onClick={(event) => event.stopPropagation()}
          onChange={() => toggleSkill(row.skill!.id)}
          className="h-3.5 w-3.5 shrink-0 accent-[var(--color-acc)] disabled:opacity-40"
        />
      );
    }
    return null;
  };
  const renderTrailing = (row: TreeRow) =>
    row.skill && !isExportable(row.skill) ? (
      <span
        className="flex shrink-0 items-center gap-1 text-fg-4"
        style={{ fontSize: "9.5px" }}
        data-testid={`export-locked-${row.skill.id}`}
      >
        <Lock size={9} />
        Managed by PDO
      </span>
    ) : null;

  const seededNames = seeded.map((skill) => skill.name).sort();

  return (
    <div
      className="fixed inset-0 z-[60] flex items-center justify-center bg-black/50"
      onClick={(event) => {
        event.stopPropagation();
        if (!exporting) onClose();
      }}
      data-testid="export-skills-backdrop"
    >
      <div
        className="flex w-[1020px] max-w-[96vw] max-h-[88vh] flex-col rounded-lg border border-line bg-bg-4 shadow-xl"
        onClick={(event) => event.stopPropagation()}
        role="dialog"
        aria-label="Export skills to an archive"
        data-testid="export-skills-modal"
      >
        <div className="flex items-center justify-between border-b border-line px-4 py-3">
          <h3 className="flex items-center gap-2 font-semibold text-fg" style={{ fontSize: "13.5px" }}>
            <Upload size={14} className="text-fg-3" />
            Export skills to an archive
          </h3>
          <button
            type="button"
            onClick={onClose}
            aria-label="Close export popup"
            className="grid h-6 w-6 place-items-center rounded text-fg-3 hover:bg-bg-5 hover:text-fg"
          >
            <X size={14} />
          </button>
        </div>

        <div className="flex min-h-0 flex-1 gap-4 p-4">
          {/* Left: the bank with checkboxes */}
          <div className="flex min-h-[420px] min-w-0 flex-1 flex-col rounded-md border border-line bg-bg-1">
            <div className="flex items-center gap-2 border-b border-line px-3 py-2" style={{ fontSize: "11px" }}>
              <span className="font-medium text-fg" data-testid="export-count">
                {checked.size} of {plural(exportable.length, "skill")} checked
              </span>
              {seeded.length > 0 && (
                <span className="text-fg-4" data-testid="export-seeded-count">
                  · {seeded.length} managed by PDO, not exportable
                </span>
              )}
              <span className="flex-1" />
              <label className="flex items-center gap-1.5 rounded-md border border-line-strong bg-bg-3 px-2 py-1">
                <Search size={11} className="text-fg-4" />
                <input
                  value={filter}
                  onChange={(event) => setFilter(event.target.value)}
                  placeholder="Filter…"
                  aria-label="Filter skills to export"
                  data-testid="export-filter"
                  className="w-[120px] bg-transparent text-fg outline-none placeholder:text-fg-4"
                  style={{ fontSize: "11px" }}
                />
              </label>
              <button type="button" onClick={selectAll} data-testid="export-select-all" className="text-acc hover:underline">
                Select all
              </button>
              <span className="text-fg-5">·</span>
              <button type="button" onClick={deselectAll} data-testid="export-deselect-all" className="text-acc hover:underline">
                Deselect all
              </button>
            </div>
            <div
              className="flex min-h-0 flex-1 flex-col overflow-y-auto py-1"
              onMouseDownCapture={() => {
                pointerRef.current = true;
              }}
              onKeyDownCapture={() => {
                pointerRef.current = false;
              }}
            >
              <SkillTree
                folders={folders}
                skills={skills}
                selected={selected}
                onSelect={(ref) => {
                  setSelected(ref);
                  // A click on a row's name toggles it (the name is the target
                  // everyone aims at, #824); arrow keys only move the selection.
                  if (pointerRef.current) {
                    pointerRef.current = false;
                    toggleRef(ref);
                  }
                }}
                expanded={expanded}
                onToggle={(id) =>
                  setExpanded((prev) => {
                    const next = new Set(prev);
                    if (next.has(id)) next.delete(id);
                    else next.add(id);
                    return next;
                  })
                }
                filter={filter}
                renderLeading={renderLeading}
                renderTrailing={renderTrailing}
                draggable={false}
                emptyState={
                  <div className="px-4 py-6 text-fg-4" style={{ fontSize: "11px" }}>
                    {filter.trim() ? `No skill matches “${filter.trim()}”.` : "The bank is empty."}
                  </div>
                }
              />
            </div>
            <p className="border-t border-line px-3 py-2 text-fg-4" style={{ fontSize: "10px" }}>
              Checking a folder checks its skills now — a snapshot: a skill added to it later is not in this archive.
            </p>
          </div>

          {/* Right: file name and summary */}
          <div className="flex w-[330px] shrink-0 flex-col gap-4 overflow-y-auto">
            <section>
              <h4 className="mb-2 text-fg-4 uppercase tracking-wide" style={{ fontSize: "10px" }}>
                File name
              </h4>
              <div className="flex items-center gap-2 rounded-md border border-line-strong bg-bg-1 px-2.5 py-1.5 focus-within:border-acc">
                <FileArchive size={12} className="shrink-0 text-fg-4" />
                <input
                  value={nameInput}
                  onChange={(event) => setTypedName(event.target.value)}
                  aria-label="File name"
                  data-testid="export-file-name"
                  disabled={exporting}
                  className="min-w-0 flex-1 bg-transparent font-mono text-fg outline-none"
                  style={{ fontSize: "11.5px" }}
                />
                {typedName !== null && (
                  <button
                    type="button"
                    onClick={() => setTypedName(null)}
                    aria-label="Reset to the default name"
                    title={`Back to ${defaultName}`}
                    data-testid="export-file-name-reset"
                    className="grid h-5 w-5 place-items-center rounded text-fg-4 hover:text-fg"
                  >
                    <RotateCcw size={11} />
                  </button>
                )}
              </div>
              <p className="mt-1.5 text-fg-4" style={{ fontSize: "10.5px" }} data-testid="export-file-name-hint">
                {nameMissing ? (
                  <span className="text-st-failed">name required</span>
                ) : appended ? (
                  <>
                    Saved as <span className="font-mono text-fg-3">{resolvedName}</span> — .zip is appended.
                  </>
                ) : typedName === null ? (
                  "Follows your selection until you type: named after the folder when you export exactly one."
                ) : (
                  "Your name is kept; ↺ goes back to the default."
                )}
              </p>
            </section>

            <section>
              <h4 className="mb-2 text-fg-4 uppercase tracking-wide" style={{ fontSize: "10px" }}>
                Will export
              </h4>
              <div className="overflow-hidden rounded-md border border-line" style={{ fontSize: "11px" }} data-testid="export-summary">
                <SummaryRow label={plural(checked.size, "skill")}>
                  {checked.size === 0 ? (
                    <span className="text-fg-4">nothing checked</span>
                  ) : (
                    checkedSkills.map((skill) => skill.name).join(" · ")
                  )}
                </SummaryRow>
                <SummaryRow label="files">
                  {checked.size === 0
                    ? "—"
                    : knownFiles
                      ? `SKILL.md${referenceFiles > 0 ? ` + ${plural(referenceFiles, "reference file")}` : ""}, verbatim`
                      : "SKILL.md + reference files, verbatim"}
                </SummaryRow>
                <SummaryRow label="layout">
                  flat: one folder per skill, plus <span className="font-mono">pdo-skills.json</span> carrying the ids
                </SummaryRow>
              </div>
            </section>

            <section>
              <h4 className="mb-2 text-fg-4 uppercase tracking-wide" style={{ fontSize: "10px" }}>
                Not exported
              </h4>
              <p className="text-fg-4" style={{ fontSize: "10.5px" }} data-testid="export-not-exported">
                {seeded.length > 0
                  ? `${plural(seeded.length, "skill")} managed by PDO (${seededNames.join(", ")}): every instance seeds itself. `
                  : ""}
                Folders are not exported — the importer picks where the skills land.
              </p>
            </section>

            <section>
              <h4 className="mb-2 text-fg-4 uppercase tracking-wide" style={{ fontSize: "10px" }}>
                On the other side
              </h4>
              <p className="text-fg-4" style={{ fontSize: "10.5px" }}>
                Settings › Skills › Add › Import from a source…, then drop the file. The ids travel with the skills, so a
                pipeline imported afterwards finds them.
              </p>
            </section>

            {error && (
              <div
                role="alert"
                className="rounded-md border border-st-failed/50 bg-st-failed-bg px-3 py-2 text-fg-2"
                style={{ fontSize: "11px" }}
                data-testid="export-error"
              >
                {error}
              </div>
            )}
          </div>
        </div>

        <div className="flex items-center justify-between gap-3 border-t border-line px-4 py-3">
          <span className="text-fg-4" style={{ fontSize: "10.5px" }}>
            Space toggles · ⌘A selects all · ⌘↵ exports · the bank is not modified
          </span>
          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={onClose}
              data-testid="export-cancel"
              className="rounded-md border border-line-strong bg-bg-3 px-3 py-1.5 text-fg-2 hover:bg-bg-4"
              style={{ fontSize: "11.5px" }}
            >
              Cancel
            </button>
            <button
              type="button"
              onClick={() => void submit()}
              disabled={!canExport}
              data-testid="export-submit"
              className="flex items-center gap-1.5 rounded-md bg-acc px-3 py-1.5 font-medium text-bg-1 transition-opacity hover:opacity-90 disabled:opacity-40"
              style={{ fontSize: "11.5px" }}
            >
              <Upload size={12} />
              {exporting ? "Exporting…" : `Export ${plural(checked.size, "skill")}`}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

function SummaryRow({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex gap-3 border-t border-line px-3 py-1.5 first:border-t-0">
      <span className="w-[84px] shrink-0 text-fg-4">{label}</span>
      <span className="min-w-0 flex-1 text-fg-2">{children}</span>
    </div>
  );
}
