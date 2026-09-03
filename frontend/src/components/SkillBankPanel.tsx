import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  ChevronDown,
  ClipboardPaste,
  Copy,
  Download,
  FileText,
  Folder,
  FolderInput,
  FolderPlus,
  MoreVertical,
  Pencil,
  Plus,
  RefreshCw,
  Search,
  Trash2,
} from "lucide-react";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import {
  ApiError,
  createSkillFolder,
  deleteSkill,
  deleteSkillFolder,
  fetchSkill,
  fetchSkillReferents,
  rescanSkillFolder,
  updateSkill,
  updateSkillFolder,
  updateSkillFolderFromSource,
} from "../api";
import type {
  Skill,
  SkillBank,
  SkillDetail,
  SkillFolder,
  SkillReferents,
  SkillRescanReport,
  SkillUpdateEntry,
} from "../types";
import { formatSize, timeAgo } from "../lib/skillMd";
import { displaySourceUrl, shortCommit } from "../lib/skillSource";
import {
  descendantFolderIds,
  folderCounts,
  folderPathLabel,
  sameRef,
  shortId,
  type TreeNodeRef,
  type TreeRow,
} from "../lib/skillTree";
import SkillTree from "./SkillTree";
import PasteSkillModal from "./PasteSkillModal";
import ImportSkillsModal from "./ImportSkillsModal";

const REMARK_PLUGINS = [remarkGfm];
const UNDO_MS = 6000;

interface Props {
  bank: SkillBank;
  loaded: boolean;
  /** Host `$HOME`, to shorten the disk path in the footer. `null` shows it as is. */
  home: string | null;
  /** Called after every successful write; the parent refetches and fires the bus. */
  onChanged: () => Promise<void>;
}

type Pending =
  | { kind: "delete-skill"; skill: Skill; referents: SkillReferents }
  | { kind: "delete-folder"; folder: SkillFolder }
  /** Update from source (#670): the re-scan is running, then its diff awaits confirmation. */
  | { kind: "rescan-folder"; folder: SkillFolder }
  | { kind: "update-folder"; folder: SkillFolder; report: SkillRescanReport };

interface Toast {
  message: string;
  undo?: () => Promise<void>;
}

function relativise(path: string, home: string | null): string {
  if (home && path.startsWith(home + "/")) return "~" + path.slice(home.length);
  return path;
}

/**
 * The Banque de skills drill-down (#668): a 300 px tree on the left, a read-only
 * detail on the right. Every gesture commits immediately (POST/PUT/DELETE) as
 * the agent profiles do, so Back / Esc / close never lose anything; the only
 * unsaved state is the paste popup's text, which asks before closing.
 *
 * Errors are shown in place — a red check in the popup, a collision under the
 * row being renamed — never as a modal on top of the modal. Move and rename are
 * undoable by toast (6 s); create and delete are not (delete goes through the
 * referents confirmation, the pattern of agent profiles).
 */
export default function SkillBankPanel({ bank, loaded, home, onChanged }: Props) {
  const { skills, folders } = bank;
  const [selected, setSelected] = useState<TreeNodeRef | null>(null);
  const [expanded, setExpanded] = useState<Set<string>>(() => new Set());
  const [filter, setFilter] = useState("");
  const [pasteOpen, setPasteOpen] = useState(false);
  const [importOpen, setImportOpen] = useState(false);
  const [addMenuOpen, setAddMenuOpen] = useState(false);
  const [detail, setDetail] = useState<SkillDetail | null>(null);
  const [detailTab, setDetailTab] = useState<"skill" | "files">("skill");
  const [renaming, setRenaming] = useState<TreeNodeRef | null>(null);
  const [renameError, setRenameError] = useState<string | null>(null);
  const [pending, setPending] = useState<Pending | null>(null);
  const [menuFor, setMenuFor] = useState<TreeNodeRef | null>(null);
  const [movePickerFor, setMovePickerFor] = useState<string | null>(null);
  const [toast, setToast] = useState<Toast | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const toastTimer = useRef<number | null>(null);

  const skillById = useMemo(() => new Map(skills.map((skill) => [skill.id, skill])), [skills]);
  const folderById = useMemo(() => new Map(folders.map((folder) => [folder.id, folder])), [folders]);
  const counts = useMemo(() => folderCounts(folders, skills), [folders, skills]);
  const existingNames = useMemo(() => skills.map((skill) => skill.name), [skills]);

  const selectedSkill = selected?.kind === "skill" ? skillById.get(selected.id) ?? null : null;
  const selectedFolder = selected?.kind === "folder" ? folderById.get(selected.id) ?? null : null;
  // A selection whose row vanished (deleted elsewhere, refetch) reads as nothing.
  const selectedRef = selectedSkill || selectedFolder ? selected : null;

  // Fetch the detail of the selected skill; key on id + updated_at so a rename
  // refreshes the header without a manual refetch.
  const selectedSkillId = selectedSkill?.id ?? null;
  const selectedSkillVersion = selectedSkill?.updated_at ?? null;
  // `detail` may lag the selection by one fetch; readers compare `detail.id`
  // with the selected skill instead of clearing it here.
  useEffect(() => {
    if (!selectedSkillId) return;
    let cancelled = false;
    fetchSkill(selectedSkillId)
      .then((result) => {
        if (!cancelled) setDetail(result);
      })
      .catch((cause) => {
        if (!cancelled) setError(cause instanceof Error ? cause.message : "Failed to load the skill");
      });
    return () => {
      cancelled = true;
    };
  }, [selectedSkillId, selectedSkillVersion]);

  // Close a kebab on outside click / Escape.
  useEffect(() => {
    if (!menuFor && !movePickerFor && !addMenuOpen) return;
    const close = () => {
      setMenuFor(null);
      setMovePickerFor(null);
      setAddMenuOpen(false);
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") close();
    };
    window.addEventListener("click", close);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("click", close);
      window.removeEventListener("keydown", onKey);
    };
  }, [menuFor, movePickerFor, addMenuOpen]);

  const showToast = useCallback((next: Toast) => {
    if (toastTimer.current) window.clearTimeout(toastTimer.current);
    setToast(next);
    toastTimer.current = window.setTimeout(() => setToast(null), UNDO_MS);
  }, []);
  useEffect(
    () => () => {
      if (toastTimer.current) window.clearTimeout(toastTimer.current);
    },
    [],
  );

  const failWith = (cause: unknown, fallback: string) =>
    setError(cause instanceof Error ? cause.message : fallback);

  const expandTo = (folderId: string | null) => {
    if (!folderId) return;
    setExpanded((prev) => {
      const next = new Set(prev);
      let cursor: string | null = folderId;
      let hops = 0;
      while (cursor && hops++ < 100) {
        next.add(cursor);
        cursor = folderById.get(cursor)?.parent_id ?? null;
      }
      return next;
    });
  };

  // ---- gestures -----------------------------------------------------------

  const moveSkill = async (skillId: string, folderId: string | null, withUndo = true) => {
    const skill = skillById.get(skillId);
    if (!skill) return;
    if ((skill.folder_id ?? null) === folderId) return;
    const from = skill.folder_id ?? null;
    setError(null);
    try {
      await updateSkill(skillId, { folder_id: folderId });
      await onChanged();
      expandTo(folderId);
      if (withUndo) {
        showToast({
          message: `Moved ${skill.name} to ${folderId ? folderById.get(folderId)?.name ?? "folder" : "the root"}`,
          undo: async () => {
            await updateSkill(skillId, { folder_id: from });
            await onChanged();
            expandTo(from);
          },
        });
      }
    } catch (cause) {
      failWith(cause, "Failed to move the skill");
    }
  };

  const moveFolder = async (folderId: string, parentId: string | null) => {
    const folder = folderById.get(folderId);
    if (!folder || (folder.parent_id ?? null) === parentId) return;
    setError(null);
    try {
      await updateSkillFolder(folderId, { parent_id: parentId });
      await onChanged();
      expandTo(parentId);
    } catch (cause) {
      failWith(cause, "Failed to move the folder");
    }
  };

  const commitRename = async (ref: TreeNodeRef, name: string) => {
    setRenameError(null);
    try {
      if (ref.kind === "skill") {
        const before = skillById.get(ref.id);
        await updateSkill(ref.id, { name });
        setRenaming(null);
        await onChanged();
        if (before) {
          showToast({
            message: `Renamed ${before.name} to ${name}`,
            undo: async () => {
              await updateSkill(ref.id, { name: before.name });
              await onChanged();
            },
          });
        }
      } else {
        const before = folderById.get(ref.id);
        await updateSkillFolder(ref.id, { name });
        setRenaming(null);
        await onChanged();
        if (before) {
          showToast({
            message: `Renamed folder ${before.name} to ${name}`,
            undo: async () => {
              await updateSkillFolder(ref.id, { name: before.name });
              await onChanged();
            },
          });
        }
      }
    } catch (cause) {
      // Stay in edit: the collision (409) reads under the row.
      setRenameError(
        cause instanceof ApiError && cause.status === 409
          ? `${name} is already taken (names are case-insensitive).`
          : cause instanceof Error
            ? cause.message
            : "Rename failed",
      );
    }
  };

  const startRename = (ref: TreeNodeRef) => {
    setMenuFor(null);
    setRenameError(null);
    setSelected(ref);
    setRenaming(ref);
  };

  const newFolder = async (parentId: string | null) => {
    setMenuFor(null);
    setError(null);
    try {
      const folder = await createSkillFolder({ name: "New folder", parent_id: parentId });
      await onChanged();
      expandTo(parentId);
      const ref: TreeNodeRef = { kind: "folder", id: folder.id };
      setSelected(ref);
      setRenaming(ref);
    } catch (cause) {
      failWith(cause, "Failed to create the folder");
    }
  };

  const askDelete = async (ref: TreeNodeRef) => {
    setMenuFor(null);
    setError(null);
    if (ref.kind === "skill") {
      const skill = skillById.get(ref.id);
      if (!skill) return;
      try {
        const referents = await fetchSkillReferents(skill.id);
        setSelected(ref);
        setPending({ kind: "delete-skill", skill, referents });
      } catch (cause) {
        failWith(cause, "Failed to load referents");
      }
    } else {
      const folder = folderById.get(ref.id);
      if (!folder) return;
      setSelected(ref);
      setPending({ kind: "delete-folder", folder });
    }
  };

  const confirmDelete = async () => {
    if (!pending) return;
    setError(null);
    try {
      if (pending.kind === "delete-skill") {
        await deleteSkill(pending.skill.id);
      } else {
        await deleteSkillFolder(pending.folder.id);
      }
      setPending(null);
      setSelected(null);
      await onChanged();
    } catch (cause) {
      failWith(cause, "Delete failed");
    }
  };

  const copyId = async (id: string) => {
    setMenuFor(null);
    try {
      await navigator.clipboard?.writeText(id);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    } catch {
      /* clipboard unavailable: nothing to do */
    }
  };

  const startUpdateFromSource = async (folder: SkillFolder) => {
    setError(null);
    setSelected({ kind: "folder", id: folder.id });
    setPending({ kind: "rescan-folder", folder });
    const scanId = newScanId();
    try {
      const report = await rescanSkillFolder(folder.id, scanId);
      setPending((current) =>
        current?.kind === "rescan-folder" && current.folder.id === folder.id
          ? { kind: "update-folder", folder, report }
          : current,
      );
    } catch (cause) {
      setPending((current) => (current?.kind === "rescan-folder" ? null : current));
      failWith(cause, "Failed to re-scan the source");
    }
  };

  const confirmUpdateFromSource = async (items: { path: string; action: "update" | "import" }[]) => {
    if (pending?.kind !== "update-folder") return;
    const folder = pending.folder;
    setError(null);
    try {
      const report = await updateSkillFolderFromSource(folder.id, { scan_id: pending.report.scan_id, items });
      setPending(null);
      await onChanged();
      expandTo(folder.id);
      setSelected({ kind: "folder", id: folder.id });
      const n = report.imported.length;
      showToast({
        message:
          report.failed.length > 0
            ? `Updated ${n} skill${n === 1 ? "" : "s"} · ${report.failed.length} failed: ${report.failed[0].error}`
            : `Updated ${n} skill${n === 1 ? "" : "s"} from ${displaySourceUrl(folder.source?.url ?? "the source")}`,
      });
    } catch (cause) {
      failWith(cause, "Update from source failed");
    }
  };

  const openSourceFolder = (folderId: string) => {
    expandTo(folderId);
    setSelected({ kind: "folder", id: folderId });
    setPending(null);
  };

  // ---- rendering ----------------------------------------------------------

  const rootLabel = bank.root_path ? `${relativise(bank.root_path, home)}/<id>/` : "";

  const renderActions = (row: TreeRow) => {
    const ref = row.ref;
    const isMenuOpen = sameRef(menuFor, ref);
    return (
      <>
        <button
          type="button"
          aria-label={`Rename ${row.folder?.name ?? row.skill?.name ?? ""}`}
          onClick={() => startRename(ref)}
          className="grid h-5 w-5 place-items-center rounded text-fg-4 hover:bg-bg-4 hover:text-fg"
        >
          <Pencil size={11} />
        </button>
        <span className="relative">
          <button
            type="button"
            aria-label={`More actions for ${row.folder?.name ?? row.skill?.name ?? ""}`}
            aria-expanded={isMenuOpen}
            data-testid={`kebab-${ref.kind}-${ref.id}`}
            onClick={(event) => {
              event.stopPropagation();
              setMovePickerFor(null);
              setMenuFor(isMenuOpen ? null : ref);
            }}
            className="grid h-5 w-5 place-items-center rounded text-fg-4 hover:bg-bg-4 hover:text-fg"
          >
            <MoreVertical size={11} />
          </button>
          {isMenuOpen && (
            <div
              role="menu"
              data-testid="tree-menu"
              className="absolute right-0 top-6 z-10 w-48 rounded-md border border-line bg-bg-4 p-1 shadow-xl"
              onClick={(event) => event.stopPropagation()}
            >
              {ref.kind === "folder" ? (
                <>
                  <MenuItem icon={<FolderPlus size={12} />} label="New subfolder" onClick={() => void newFolder(ref.id)} />
                  <MenuItem icon={<Pencil size={12} />} label="Rename" hint="F2" onClick={() => startRename(ref)} />
                  <MenuSeparator />
                  <MenuItem icon={<Trash2 size={12} />} label="Delete folder" hint="⌫" danger onClick={() => void askDelete(ref)} />
                </>
              ) : (
                <>
                  <MenuItem icon={<Pencil size={12} />} label="Rename" hint="F2" onClick={() => startRename(ref)} />
                  <MenuItem
                    icon={<FolderInput size={12} />}
                    label="Move to…"
                    onClick={() => {
                      setMenuFor(null);
                      setSelected(ref);
                      setMovePickerFor(ref.id);
                    }}
                  />
                  <MenuItem icon={<Copy size={12} />} label="Copy id" onClick={() => void copyId(ref.id)} />
                  <MenuSeparator />
                  <MenuItem icon={<Trash2 size={12} />} label="Delete…" hint="⌫" danger onClick={() => void askDelete(ref)} />
                </>
              )}
            </div>
          )}
        </span>
      </>
    );
  };

  const emptyBank = loaded && skills.length === 0 && folders.length === 0;

  return (
    <div className="flex min-h-0 flex-1" data-testid="skill-bank-panel">
      {/* Left: tree */}
      <div className="flex w-[300px] shrink-0 flex-col border-r border-line">
        <div className="flex items-center gap-2 px-3 py-2.5">
          <label className="flex min-w-0 flex-1 items-center gap-1.5 rounded-md border border-line-strong bg-bg-3 px-2 py-1.5">
            <Search size={12} className="shrink-0 text-fg-4" />
            <input
              value={filter}
              onChange={(event) => setFilter(event.target.value)}
              placeholder="Filter skills…"
              aria-label="Filter skills"
              data-testid="skill-filter"
              className="min-w-0 flex-1 bg-transparent text-fg outline-none placeholder:text-fg-4"
              style={{ fontSize: "11.5px" }}
            />
          </label>
          <button
            type="button"
            onClick={() => void newFolder(selectedFolder?.id ?? (selectedSkill?.folder_id ?? null))}
            aria-label="New folder"
            title="New folder"
            data-testid="skill-new-folder"
            className="flex shrink-0 items-center gap-1 rounded-md border border-line-strong bg-bg-3 px-2 py-1.5 text-fg-2 hover:border-acc"
          >
            <Folder size={12} />
            <span style={{ fontSize: "12px", lineHeight: 1 }}>+</span>
          </button>
          <span className="relative">
            <button
              type="button"
              onClick={(event) => {
                event.stopPropagation();
                setMenuFor(null);
                setAddMenuOpen((open) => !open);
              }}
              aria-haspopup="menu"
              aria-expanded={addMenuOpen}
              data-testid="skill-add"
              className="flex shrink-0 items-center gap-1 rounded-md bg-acc px-2.5 py-1.5 font-medium text-bg-1 hover:opacity-90"
              style={{ fontSize: "11.5px" }}
            >
              <Plus size={12} />
              Add
              <ChevronDown size={11} />
            </button>
            {addMenuOpen && (
              <div
                role="menu"
                data-testid="skill-add-menu"
                className="absolute right-0 top-8 z-10 w-64 rounded-md border border-line bg-bg-4 p-1 shadow-xl"
                onClick={(event) => event.stopPropagation()}
              >
                <MenuItem
                  icon={<ClipboardPaste size={12} />}
                  label="Paste SKILL.md…"
                  testid="skill-paste"
                  onClick={() => {
                    setAddMenuOpen(false);
                    setPasteOpen(true);
                  }}
                />
                <MenuItem
                  icon={<Download size={12} />}
                  label="Import from a source…"
                  hint="repo · folder"
                  testid="skill-import"
                  onClick={() => {
                    setAddMenuOpen(false);
                    setImportOpen(true);
                  }}
                />
              </div>
            )}
          </span>
        </div>

        <div className="flex min-h-0 flex-1 flex-col overflow-y-auto">
          <SkillTree
            folders={folders}
            skills={skills}
            selected={selectedRef}
            onSelect={(ref) => {
              setSelected(ref);
              setPending(null);
              setMenuFor(null);
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
            renaming={renaming}
            renameError={renameError}
            onRenameCommit={commitRename}
            onRenameCancel={() => {
              setRenaming(null);
              setRenameError(null);
            }}
            onDropSkill={(skillId, folderId) => void moveSkill(skillId, folderId)}
            renderActions={renderActions}
            onRequestRename={startRename}
            onRequestDelete={(ref) => void askDelete(ref)}
            emptyState={
              !loaded ? (
                <div className="px-4 py-6 text-fg-4" style={{ fontSize: "11px" }}>
                  Loading…
                </div>
              ) : filter.trim() ? (
                <div className="px-4 py-6 text-fg-4" style={{ fontSize: "11px" }}>
                  No skill matches “{filter.trim()}”.
                </div>
              ) : null
            }
          />
        </div>

        <div
          className="flex items-center justify-between gap-2 border-t border-line px-3 py-2 text-fg-4"
          style={{ fontSize: "10.5px" }}
          data-testid="skill-bank-footer"
        >
          <span className="shrink-0 whitespace-nowrap">
            {skills.length} skill{skills.length === 1 ? "" : "s"} · {folders.length} folder
            {folders.length === 1 ? "" : "s"}
          </span>
          {rootLabel && (
            <span className="truncate font-mono" title={bank.root_path}>
              {rootLabel}
            </span>
          )}
        </div>
      </div>

      {/* Right: detail / confirmation / empty state */}
      <div className="relative flex min-h-0 min-w-0 flex-1 flex-col overflow-y-auto">
        {error && (
          <div
            role="alert"
            className="m-4 mb-0 rounded-md border border-st-failed/50 bg-st-failed-bg px-3 py-2 text-fg-2"
            style={{ fontSize: "11px" }}
            data-testid="skill-bank-error"
          >
            {error}
          </div>
        )}

        {pending?.kind === "delete-skill" ? (
          <DeleteSkillConfirm
            skill={pending.skill}
            referents={pending.referents}
            path={relativise(`${bank.root_path}/${pending.skill.id}`, home)}
            onCancel={() => setPending(null)}
            onConfirm={() => void confirmDelete()}
          />
        ) : pending?.kind === "delete-folder" ? (
          <DeleteFolderConfirm
            folder={pending.folder}
            count={counts.get(pending.folder.id) ?? 0}
            parentName={pending.folder.parent_id ? folderById.get(pending.folder.parent_id)?.name ?? null : null}
            onCancel={() => setPending(null)}
            onConfirm={() => void confirmDelete()}
          />
        ) : pending?.kind === "rescan-folder" ? (
          <RescanInProgress folder={pending.folder} onCancel={() => setPending(null)} />
        ) : pending?.kind === "update-folder" ? (
          <UpdateFromSourceConfirm
            folder={pending.folder}
            report={pending.report}
            onCancel={() => setPending(null)}
            onConfirm={(items) => void confirmUpdateFromSource(items)}
          />
        ) : selectedSkill ? (
          <SkillDetailView
            skill={selectedSkill}
            detail={detail}
            folders={folders}
            tab={detailTab}
            onTab={setDetailTab}
            copied={copied}
            onCopyId={() => void copyId(selectedSkill.id)}
            onRename={() => startRename({ kind: "skill", id: selectedSkill.id })}
            onMove={() => setMovePickerFor(selectedSkill.id)}
            onDelete={() => void askDelete({ kind: "skill", id: selectedSkill.id })}
            movePickerOpen={movePickerFor === selectedSkill.id}
            onMoveTo={(folderId) => {
              setMovePickerFor(null);
              void moveSkill(selectedSkill.id, folderId);
            }}
            sourceFolder={folders.find(
              (folder) =>
                folder.source &&
                selectedSkill.source &&
                folder.source.url === selectedSkill.source.url &&
                selectedSkill.source.path.startsWith(folder.source.path),
            ) ?? null}
            onOpenSourceFolder={openSourceFolder}
          />
        ) : selectedFolder ? (
          <FolderDetailView
            folder={selectedFolder}
            folders={folders}
            skills={skills}
            count={counts.get(selectedFolder.id) ?? 0}
            onNewSubfolder={() => void newFolder(selectedFolder.id)}
            onRename={() => startRename({ kind: "folder", id: selectedFolder.id })}
            onDelete={() => void askDelete({ kind: "folder", id: selectedFolder.id })}
            onMoveTo={(parentId) => void moveFolder(selectedFolder.id, parentId)}
            onUpdateFromSource={() => void startUpdateFromSource(selectedFolder)}
          />
        ) : emptyBank ? (
          <EmptyBank onPaste={() => setPasteOpen(true)} onImport={() => setImportOpen(true)} />
        ) : (
          <div className="flex flex-1 items-center justify-center p-8 text-center text-fg-4" style={{ fontSize: "11.5px" }}>
            Select a skill to read it, or a folder to manage it.
          </div>
        )}

        {toast && (
          <div
            role="status"
            data-testid="skill-toast"
            className="absolute bottom-3 left-1/2 flex -translate-x-1/2 items-center gap-3 rounded-md border border-line bg-bg-3 px-3 py-2 text-fg-2 shadow-xl"
            style={{ fontSize: "11.5px" }}
          >
            <span>{toast.message}</span>
            {toast.undo && (
              <button
                type="button"
                data-testid="skill-toast-undo"
                onClick={() => {
                  const undo = toast.undo;
                  setToast(null);
                  if (toastTimer.current) window.clearTimeout(toastTimer.current);
                  void undo?.().catch((cause) => failWith(cause, "Undo failed"));
                }}
                className="font-medium text-acc hover:underline"
              >
                Undo
              </button>
            )}
          </div>
        )}
      </div>

      {importOpen && (
        <ImportSkillsModal
          folders={folders}
          existingNames={existingNames}
          initialFolderId={selectedFolder?.id ?? selectedSkill?.folder_id ?? null}
          home={home}
          onClose={() => setImportOpen(false)}
          onImported={async (report, complete) => {
            await onChanged();
            expandTo(report.folder.id);
            setSelected({ kind: "folder", id: report.folder.id });
            setPending(null);
            if (complete) {
              const n = report.imported.length;
              showToast({ message: `Imported ${n} skill${n === 1 ? "" : "s"} into ${report.folder.name}` });
            }
          }}
        />
      )}

      {pasteOpen && (
        <PasteSkillModal
          folders={folders}
          existingNames={existingNames}
          initialFolderId={selectedFolder?.id ?? selectedSkill?.folder_id ?? null}
          onClose={() => setPasteOpen(false)}
          onCreated={async (skill) => {
            await onChanged();
            expandTo(skill.folder_id ?? null);
            setSelected({ kind: "skill", id: skill.id });
            setDetailTab("skill");
          }}
        />
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------

function newScanId(): string {
  const c = globalThis.crypto as Crypto | undefined;
  if (c && typeof c.randomUUID === "function") return c.randomUUID();
  return `scan-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
}

function MenuItem({
  icon,
  label,
  hint,
  danger,
  testid,
  onClick,
}: {
  icon: React.ReactNode;
  label: string;
  hint?: string;
  danger?: boolean;
  testid?: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      role="menuitem"
      data-testid={testid}
      onClick={onClick}
      className={`flex w-full items-center gap-2 rounded px-2 py-1.5 text-left hover:bg-bg-5 ${
        danger ? "text-st-failed" : "text-fg-2"
      }`}
      style={{ fontSize: "11.5px" }}
    >
      <span className="shrink-0">{icon}</span>
      <span className="flex-1 whitespace-nowrap">{label}</span>
      {hint && (
        <span className="font-mono text-fg-4" style={{ fontSize: "10px" }}>
          {hint}
        </span>
      )}
    </button>
  );
}

function MenuSeparator() {
  return <div className="my-1 border-t border-line" />;
}

function EmptyBank({ onPaste, onImport }: { onPaste: () => void; onImport: () => void }) {
  return (
    <div className="flex flex-1 flex-col items-center justify-center gap-3 p-8 text-center" data-testid="skill-bank-empty">
      <div className="grid h-14 w-14 place-items-center rounded-xl border border-dashed border-line-strong text-fg-4">
        <FileText size={20} />
      </div>
      <h3 className="font-semibold text-fg" style={{ fontSize: "14px" }}>
        No skills yet
      </h3>
      <p className="max-w-[420px] text-fg-3" style={{ fontSize: "12px" }}>
        Import a repo of skills, or paste the text of a <span className="font-mono">SKILL.md</span>. Skills are
        delivered to every worktree PDO creates, never committed.
      </p>
      <div className="mt-1 flex items-center gap-2">
        <button
          type="button"
          onClick={onImport}
          data-testid="skill-import-empty"
          className="flex items-center gap-1.5 rounded-md bg-acc px-3 py-2 font-medium text-bg-1 hover:opacity-90"
          style={{ fontSize: "12px" }}
        >
          <Download size={13} />
          Import from a source
        </button>
        <button
          type="button"
          onClick={onPaste}
          data-testid="skill-paste-empty"
          className="flex items-center gap-1.5 rounded-md border border-line-strong bg-bg-3 px-3 py-2 font-medium text-fg-2 hover:border-acc"
          style={{ fontSize: "12px" }}
        >
          <ClipboardPaste size={13} />
          Paste SKILL.md
        </button>
      </div>
    </div>
  );
}

function ActionButton({
  icon,
  label,
  onClick,
  danger,
  testid,
}: {
  icon: React.ReactNode;
  label: string;
  onClick: () => void;
  danger?: boolean;
  testid?: string;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      data-testid={testid}
      className={`flex shrink-0 items-center gap-1.5 whitespace-nowrap rounded-md border px-2.5 py-1.5 transition-colors ${
        danger
          ? "border-st-failed/40 text-st-failed hover:bg-st-failed-bg"
          : "border-line-strong bg-bg-3 text-fg-2 hover:border-acc"
      }`}
      style={{ fontSize: "11.5px" }}
    >
      {icon}
      {label}
    </button>
  );
}

function FolderPicker({
  folders,
  exclude,
  current,
  onPick,
  onClose,
}: {
  folders: SkillFolder[];
  /** Folder ids not offered (a folder cannot move under itself). */
  exclude?: Set<string>;
  current: string | null;
  onPick: (folderId: string | null) => void;
  onClose: () => void;
}) {
  const options = folders.filter((folder) => !exclude?.has(folder.id));
  return (
    <div
      role="menu"
      data-testid="move-picker"
      className="absolute left-0 top-9 z-10 w-64 rounded-md border border-line bg-bg-4 p-1 shadow-xl"
      onClick={(event) => event.stopPropagation()}
    >
      <div className="px-2 py-1 text-fg-4 uppercase tracking-wide" style={{ fontSize: "9.5px" }}>
        Move to
      </div>
      <button
        type="button"
        role="menuitem"
        onClick={() => onPick(null)}
        disabled={current === null}
        className="flex w-full items-center gap-2 rounded px-2 py-1.5 text-left text-fg-2 hover:bg-bg-5 disabled:opacity-40"
        style={{ fontSize: "11.5px" }}
      >
        <Folder size={12} /> Root of the bank
      </button>
      {options.map((folder) => (
        <button
          key={folder.id}
          type="button"
          role="menuitem"
          onClick={() => onPick(folder.id)}
          disabled={current === folder.id}
          className="flex w-full items-center gap-2 rounded px-2 py-1.5 text-left text-fg-2 hover:bg-bg-5 disabled:opacity-40"
          style={{ fontSize: "11.5px" }}
        >
          <Folder size={12} className="shrink-0" />
          <span className="truncate">{folderPathLabel(folder.id, folders)}</span>
        </button>
      ))}
      <MenuSeparator />
      <button
        type="button"
        onClick={onClose}
        className="w-full rounded px-2 py-1 text-left text-fg-4 hover:bg-bg-5"
        style={{ fontSize: "11px" }}
      >
        Cancel
      </button>
    </div>
  );
}

function SkillDetailView({
  skill,
  detail,
  folders,
  tab,
  onTab,
  copied,
  onCopyId,
  onRename,
  onMove,
  onDelete,
  movePickerOpen,
  onMoveTo,
  sourceFolder,
  onOpenSourceFolder,
}: {
  skill: Skill;
  detail: SkillDetail | null;
  folders: SkillFolder[];
  tab: "skill" | "files";
  onTab: (tab: "skill" | "files") => void;
  copied: boolean;
  onCopyId: () => void;
  onRename: () => void;
  onMove: () => void;
  onDelete: () => void;
  movePickerOpen: boolean;
  onMoveTo: (folderId: string | null) => void;
  /** The Source folder this skill was imported into, if it still exists. */
  sourceFolder: SkillFolder | null;
  onOpenSourceFolder: (folderId: string) => void;
}) {
  const current = detail && detail.id === skill.id ? detail : null;
  const fileCount = current?.files.length ?? 0;
  const frontmatter = current?.frontmatter ?? null;
  const frontmatterKeys = frontmatter
    ? [
        ...(current?.frontmatter_keys ?? []).filter((key) => key in frontmatter),
        ...Object.keys(frontmatter).filter((key) => !(current?.frontmatter_keys ?? []).includes(key)),
      ]
    : [];
  return (
    <div className="flex min-h-0 flex-1 flex-col p-5" data-testid="skill-detail">
      <div className="flex items-baseline gap-2">
        <h3 className="truncate font-semibold text-fg" style={{ fontSize: "17px" }} data-testid="skill-detail-name">
          {skill.name}
        </h3>
        <span className="flex items-center gap-1 font-mono text-fg-4" style={{ fontSize: "10.5px" }}>
          id {shortId(skill.id)}
          <button
            type="button"
            aria-label="Copy id"
            title={skill.id}
            onClick={onCopyId}
            className="grid h-4 w-4 place-items-center rounded text-fg-4 hover:text-fg"
          >
            <Copy size={10} />
          </button>
          {copied && <span className="text-acc">copied</span>}
        </span>
      </div>
      <p className="mt-1.5 text-fg-2" style={{ fontSize: "13px" }} data-testid="skill-detail-description">
        {skill.description}
      </p>
      <div className="mt-2 flex flex-wrap gap-x-4 gap-y-1 text-fg-4" style={{ fontSize: "10.5px" }}>
        <span>
          {fileCount} file{fileCount === 1 ? "" : "s"}
        </span>
        <span>updated {timeAgo(skill.updated_at)}</span>
        {skill.source ? (
          <span className="flex items-center gap-1" data-testid="skill-detail-provenance">
            imported from{" "}
            {sourceFolder ? (
              <button
                type="button"
                onClick={() => onOpenSourceFolder(sourceFolder.id)}
                className="font-mono text-fg-3 hover:text-acc hover:underline"
                title={`Open the Source folder “${sourceFolder.name}”`}
              >
                {displaySourceUrl(skill.source.url)}
                {skill.source.commit ? `@${shortCommit(skill.source.commit)}` : ""} · {skill.source.path || "."}
              </button>
            ) : (
              <span className="font-mono text-fg-3" title={skill.source.url}>
                {displaySourceUrl(skill.source.url)}
                {skill.source.commit ? `@${shortCommit(skill.source.commit)}` : ""} · {skill.source.path || "."}
              </span>
            )}
          </span>
        ) : (
          <span>created by paste</span>
        )}
        <span>referenced by 0 tiers</span>
      </div>

      <div className="mt-3 flex items-center gap-2">
        <ActionButton icon={<Pencil size={11} />} label="Rename" onClick={onRename} testid="skill-detail-rename" />
        <span className="relative">
          <ActionButton icon={<FolderInput size={11} />} label="Move to…" onClick={onMove} testid="skill-detail-move" />
          {movePickerOpen && (
            <FolderPicker
              folders={folders}
              current={skill.folder_id ?? null}
              onPick={onMoveTo}
              onClose={() => onMoveTo(skill.folder_id ?? null)}
            />
          )}
        </span>
        <span className="flex-1" />
        <ActionButton icon={<Trash2 size={11} />} label="Delete…" onClick={onDelete} danger testid="skill-detail-delete" />
      </div>

      <div className="mt-4 flex gap-4 border-b border-line" role="tablist">
        <TabButton active={tab === "skill"} onClick={() => onTab("skill")} label="SKILL.md" />
        <TabButton
          active={tab === "files"}
          onClick={() => onTab("files")}
          label={
            <>
              Files <span className="text-fg-4">· {fileCount}</span>
            </>
          }
        />
      </div>

      {tab === "skill" ? (
        <div className="mt-3 flex flex-col gap-4">
          {frontmatter && frontmatterKeys.length > 0 && (
            <div className="overflow-hidden rounded-md border border-line" data-testid="skill-frontmatter">
              <div className="flex items-center justify-between bg-bg-3 px-3 py-1.5 text-fg-4 uppercase tracking-wide" style={{ fontSize: "9.5px" }}>
                <span>Frontmatter</span>
                <span>Read-only</span>
              </div>
              <table className="w-full" style={{ fontSize: "11.5px" }}>
                <tbody>
                  {frontmatterKeys.map((key) => {
                    const value = frontmatter[key];
                    return (
                    <tr key={key} className="border-t border-line">
                      <td className="w-[160px] px-3 py-1.5 align-top font-mono text-fg-3">{key}</td>
                      <td className="px-3 py-1.5 font-mono text-fg">
                        {typeof value === "string" ? value : JSON.stringify(value)}
                      </td>
                    </tr>
                    );
                  })}
                </tbody>
              </table>
            </div>
          )}
          {current === null ? (
            <div className="text-fg-4" style={{ fontSize: "11px" }}>
              Loading…
            </div>
          ) : current.body === null ? (
            <div className="rounded-md border border-st-blocked/40 bg-st-blocked-bg px-3 py-2 text-fg-2" style={{ fontSize: "11px" }}>
              The <span className="font-mono">SKILL.md</span> of this skill is missing on disk (
              <span className="font-mono">{current.path}</span>). Delete the entry and paste it again.
            </div>
          ) : (
            <div className="artifact-markdown prose-sm text-fg-2 [&_ul]:list-disc [&_ol]:list-decimal [&_ul]:pl-5 [&_ol]:pl-5" style={{ fontSize: "12.5px" }} data-testid="skill-body">
              <Markdown remarkPlugins={REMARK_PLUGINS}>{current.body}</Markdown>
            </div>
          )}
        </div>
      ) : (
        <div className="mt-3 flex flex-col gap-1" data-testid="skill-files">
          <FileRow name="SKILL.md" size={current?.content ? new TextEncoder().encode(current.content).length : null} />
          {current?.files.map((file) => <FileRow key={file.path} name={file.path} size={file.size} />)}
          {current && current.files.length === 0 && (
            <p className="mt-2 text-fg-4" style={{ fontSize: "11px" }}>
              No reference files. Attaching files arrives in a later ticket.
            </p>
          )}
        </div>
      )}
    </div>
  );
}

function FileRow({ name, size }: { name: string; size: number | null }) {
  return (
    <div className="flex items-center gap-2 rounded-md border border-line bg-bg-3 px-3 py-2" style={{ fontSize: "11.5px" }}>
      <FileText size={12} className="shrink-0 text-fg-3" />
      <span className="font-mono text-fg">{name}</span>
      <span className="rounded border border-line px-1.5 text-fg-4" style={{ fontSize: "9.5px" }}>
        read-only
      </span>
      <span className="flex-1" />
      {size !== null && (
        <span className="font-mono text-fg-4" style={{ fontSize: "10.5px" }}>
          {formatSize(size)}
        </span>
      )}
    </div>
  );
}

function TabButton({ active, onClick, label }: { active: boolean; onClick: () => void; label: React.ReactNode }) {
  return (
    <button
      type="button"
      role="tab"
      aria-selected={active}
      onClick={onClick}
      className={`-mb-px border-b-2 px-1 pb-2 ${active ? "border-acc text-fg" : "border-transparent text-fg-3 hover:text-fg"}`}
      style={{ fontSize: "12px" }}
    >
      {label}
    </button>
  );
}

function FolderDetailView({
  folder,
  folders,
  skills,
  count,
  onNewSubfolder,
  onRename,
  onDelete,
  onMoveTo,
  onUpdateFromSource,
}: {
  folder: SkillFolder;
  folders: SkillFolder[];
  skills: Skill[];
  count: number;
  onNewSubfolder: () => void;
  onRename: () => void;
  onDelete: () => void;
  onMoveTo: (parentId: string | null) => void;
  onUpdateFromSource: () => void;
}) {
  const [picker, setPicker] = useState(false);
  const exclude = useMemo(() => descendantFolderIds(folder.id, folders), [folder.id, folders]);
  const source = folder.source ?? null;
  const importedHere = source
    ? skills.filter((skill) => skill.folder_id === folder.id && skill.source?.url === source.url).length
    : 0;
  return (
    <div className="flex min-h-0 flex-1 flex-col p-5" data-testid="folder-detail">
      <div className="flex items-center gap-2">
        <Folder size={16} className="text-st-await" />
        <h3 className="truncate font-semibold text-fg" style={{ fontSize: "17px" }}>
          {folderPathLabel(folder.id, folders)}
        </h3>
        {source && (
          <span
            className="rounded border border-acc/40 bg-acc/10 px-1.5 py-0.5 text-acc"
            style={{ fontSize: "9.5px" }}
            data-testid="folder-source-badge"
          >
            Source
          </span>
        )}
      </div>
      <p className="mt-1.5 text-fg-3" style={{ fontSize: "12px" }}>
        {count} skill{count === 1 ? "" : "s"} in this folder and below. Checking a folder in a
        selector will check its skills at that instant; nothing references a folder.
      </p>
      <div className="mt-3 flex items-center gap-2">
        <ActionButton icon={<FolderPlus size={11} />} label="New subfolder" onClick={onNewSubfolder} testid="folder-detail-new" />
        <ActionButton icon={<Pencil size={11} />} label="Rename" onClick={onRename} testid="folder-detail-rename" />
        <span className="relative">
          <ActionButton icon={<FolderInput size={11} />} label="Move to…" onClick={() => setPicker((p) => !p)} />
          {picker && (
            <FolderPicker
              folders={folders}
              exclude={exclude}
              current={folder.parent_id ?? null}
              onPick={(parentId) => {
                setPicker(false);
                onMoveTo(parentId);
              }}
              onClose={() => setPicker(false)}
            />
          )}
        </span>
        <span className="flex-1" />
        <ActionButton icon={<Trash2 size={11} />} label="Delete folder" onClick={onDelete} danger testid="folder-detail-delete" />
      </div>

      {source && (
        <>
          <div className="mt-4 overflow-hidden rounded-md border border-line" data-testid="folder-provenance">
            <div className="flex items-center justify-between bg-bg-3 px-3 py-1.5">
              <span className="text-fg-4 uppercase tracking-wide" style={{ fontSize: "9.5px" }}>
                Provenance
              </span>
              <button
                type="button"
                onClick={onUpdateFromSource}
                data-testid="folder-update-from-source"
                className="flex items-center gap-1 text-acc hover:underline"
                style={{ fontSize: "11px" }}
              >
                <RefreshCw size={10} />
                Update from source…
              </button>
            </div>
            <table className="w-full" style={{ fontSize: "11.5px" }}>
              <tbody>
                <ProvenanceRow label="source">
                  <span className="font-mono text-fg">{source.url}</span>
                </ProvenanceRow>
                <ProvenanceRow label="ref">
                  <span className="font-mono text-fg">{source.ref ?? "default branch"}</span>
                </ProvenanceRow>
                <ProvenanceRow label="commit">
                  <span className="font-mono text-fg">{source.commit ? shortCommit(source.commit) : "—"}</span>
                  {source.imported_at && (
                    <span className="ml-2 font-mono text-fg-4" style={{ fontSize: "10.5px" }}>
                      · imported {timeAgo(source.imported_at)}
                    </span>
                  )}
                </ProvenanceRow>
                <ProvenanceRow label="path">
                  <span className="font-mono text-fg">{source.path ? `${source.path}/` : "."}</span>
                </ProvenanceRow>
                <ProvenanceRow label="imported">
                  <span className="text-fg">
                    {importedHere} of {source.found} skill{source.found === 1 ? "" : "s"} found at the source
                  </span>
                  {source.invalid > 0 && (
                    <span className="ml-2 text-fg-4" style={{ fontSize: "10.5px" }}>
                      · {source.invalid} invalid
                    </span>
                  )}
                </ProvenanceRow>
              </tbody>
            </table>
          </div>
          <p className="mt-3 rounded-md border border-dashed border-line px-3 py-2 text-fg-4" style={{ fontSize: "10.5px" }}>
            Renaming or moving this folder keeps its provenance. Deleting it moves its skills to the parent and drops the
            link to the source; the skills keep their own provenance.
          </p>
        </>
      )}
    </div>
  );
}

function ProvenanceRow({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <tr className="border-t border-line">
      <td className="w-[110px] px-3 py-1.5 align-top text-fg-4">{label}</td>
      <td className="px-3 py-1.5">{children}</td>
    </tr>
  );
}

function RescanInProgress({ folder, onCancel }: { folder: SkillFolder; onCancel: () => void }) {
  const source = folder.source;
  return (
    <div className="flex min-h-0 flex-1 flex-col p-5" data-testid="folder-rescan">
      <h3 className="font-semibold text-fg" style={{ fontSize: "17px" }}>
        Update {folder.name} from its source?
      </h3>
      <p className="mt-2 text-fg-3" style={{ fontSize: "12px" }}>
        Re-scanning <span className="font-mono">{source ? displaySourceUrl(source.url) : ""}</span>
        {source?.ref ? <span className="font-mono">@{source.ref}</span> : null}… nothing is written before you confirm.
      </p>
      <div className="mt-6 flex flex-col items-center gap-3">
        <div className="h-1 w-[280px] overflow-hidden rounded-full bg-bg-5">
          <div className="h-full w-1/3 animate-pulse rounded-full bg-st-running" />
        </div>
        <span className="text-fg-4" style={{ fontSize: "10.5px" }}>
          Shallow clone with your git credentials
        </span>
        <button
          type="button"
          onClick={onCancel}
          className="rounded-md border border-line-strong bg-bg-3 px-3 py-1.5 text-fg-2 hover:bg-bg-4"
          style={{ fontSize: "11.5px" }}
        >
          Cancel
        </button>
      </div>
    </div>
  );
}

const UPDATE_BADGE: Record<SkillUpdateEntry["status"], { label: string; className: string }> = {
  updated: { label: "updated", className: "border-st-blocked/50 bg-st-blocked-bg text-st-blocked" },
  unchanged: { label: "unchanged", className: "border-line bg-bg-3 text-fg-4" },
  new: { label: "new at source · not imported", className: "border-acc/40 bg-acc/10 text-acc" },
  skipped: { label: "skipped", className: "border-line bg-bg-3 text-fg-4" },
  gone: { label: "gone from source", className: "border-st-failed/40 bg-st-failed-bg text-st-failed" },
  invalid: { label: "not importable", className: "border-st-failed/40 bg-st-failed-bg text-st-failed" },
};

/**
 * The diff of an Update from source (#670), confirmed in the right panel like
 * the delete confirmations: updated rows checked, unchanged greyed, new at the
 * source unchecked by default (an update does not widen silently), skipped for
 * a skill the user moved out, gone kept and flagged.
 */
function UpdateFromSourceConfirm({
  folder,
  report,
  onCancel,
  onConfirm,
}: {
  folder: SkillFolder;
  report: SkillRescanReport;
  onCancel: () => void;
  onConfirm: (items: { path: string; action: "update" | "import" }[]) => void;
}) {
  const [checked, setChecked] = useState<Set<string>>(
    () => new Set(report.entries.filter((entry) => entry.status === "updated").map((entry) => entry.path)),
  );
  const [submitting, setSubmitting] = useState(false);
  const items = report.entries
    .filter((entry) => checked.has(entry.path))
    .map((entry) => ({ path: entry.path, action: entry.status === "new" ? ("import" as const) : ("update" as const) }));
  const changed = report.entries.filter((e) => e.status === "updated").length;
  const sameCommit = report.previous_commit && report.commit && report.previous_commit === report.commit;
  const updatedNames = report.entries.filter((e) => checked.has(e.path) && e.status === "updated").map((e) => e.name);
  return (
    <div className="flex min-h-0 flex-1 flex-col p-5" data-testid="folder-update">
      <h3 className="font-semibold text-fg" style={{ fontSize: "17px" }}>
        Update {folder.name} from its source?
      </h3>
      <p className="mt-1.5 text-fg-3" style={{ fontSize: "12px" }} data-testid="folder-update-summary">
        Re-scanned <span className="font-mono">{displaySourceUrl(report.source.url)}</span>
        {report.source.ref ? <span className="font-mono">@{report.source.ref}</span> : null} ·{" "}
        {sameCommit ? (
          <>
            still at <span className="font-mono">{shortCommit(report.commit)}</span>
          </>
        ) : (
          <>
            <span className="font-mono">{shortCommit(report.previous_commit) || "?"}</span> →{" "}
            <span className="font-mono">{shortCommit(report.commit) || "?"}</span>
          </>
        )}{" "}
        · {changed} skill{changed === 1 ? "" : "s"} changed.
      </p>
      <ul className="mt-3 flex flex-col overflow-hidden rounded-md border border-line bg-bg-1" data-testid="folder-update-entries">
        {report.entries.map((entry) => {
          const selectable = entry.status === "updated" || (entry.status === "new" && !entry.name_taken_by);
          const badge = UPDATE_BADGE[entry.status];
          const dim = !selectable;
          return (
            <li
              key={entry.path}
              className="flex items-start gap-2.5 border-b border-line px-3 py-2 last:border-b-0"
              data-testid={`update-entry-${entry.name}`}
              data-status={entry.status}
            >
              <input
                type="checkbox"
                checked={entry.status === "unchanged" || checked.has(entry.path)}
                disabled={!selectable}
                aria-label={`${entry.status === "new" ? "Import" : "Update"} ${entry.name}`}
                onChange={(event) =>
                  setChecked((prev) => {
                    const next = new Set(prev);
                    if (event.target.checked) next.add(entry.path);
                    else next.delete(entry.path);
                    return next;
                  })
                }
                className={`mt-0.5 h-3.5 w-3.5 shrink-0 accent-[var(--color-acc)] ${dim ? "opacity-40" : ""}`}
              />
              <div className="min-w-0 flex-1">
                <div className="flex items-center gap-2">
                  <span className={`font-semibold ${dim ? "text-fg-3" : "text-fg"}`} style={{ fontSize: "12.5px" }}>
                    {entry.name}
                  </span>
                  <span className="flex-1" />
                  <span className={`rounded border px-1.5 py-0.5 whitespace-nowrap ${badge.className}`} style={{ fontSize: "9.5px" }}>
                    {badge.label}
                  </span>
                </div>
                <div className="mt-0.5 font-mono text-fg-4" style={{ fontSize: "10px" }}>
                  {entry.status === "updated"
                    ? [
                        entry.skill_md_changed ? "SKILL.md changed" : null,
                        entry.files_added ? `+${entry.files_added} reference file${entry.files_added === 1 ? "" : "s"}` : null,
                        entry.files_changed ? `${entry.files_changed} changed` : null,
                        entry.files_removed ? `−${entry.files_removed} removed` : null,
                      ]
                        .filter(Boolean)
                        .join(" · ")
                    : entry.status === "unchanged"
                      ? "identical"
                      : entry.status === "new"
                        ? entry.name_taken_by
                          ? `${entry.path}/SKILL.md · name taken by “${entry.name_taken_by}”`
                          : `${entry.path}/SKILL.md`
                        : entry.status === "skipped"
                          ? `${entry.reason ?? "moved out of this folder"} · left alone`
                          : entry.status === "gone"
                            ? entry.reason ?? "no longer at the source · kept in the bank"
                            : entry.reason ?? "invalid frontmatter"}
                </div>
              </div>
            </li>
          );
        })}
        {report.entries.length === 0 && (
          <li className="px-3 py-4 text-center text-fg-4" style={{ fontSize: "11px" }}>
            Nothing at the source.
          </li>
        )}
      </ul>
      <p className="mt-3 rounded-md border border-st-blocked/40 bg-st-blocked-bg px-3 py-2 text-fg-2" style={{ fontSize: "11px" }}>
        Runs already started keep their frozen copy.
        {updatedNames.length > 0 && (
          <>
            {" "}
            Nodes that select <strong className="text-fg">{updatedNames.join(", ")}</strong> will pick up the new content at
            their next spawn.
          </>
        )}{" "}
        Skills that vanished from the source are kept in the bank and flagged.
      </p>
      <div className="mt-4 flex justify-end gap-2">
        <button
          type="button"
          onClick={onCancel}
          className="rounded-md border border-line-strong bg-bg-3 px-3 py-1.5 text-fg-2 hover:bg-bg-4"
          style={{ fontSize: "11.5px" }}
        >
          Cancel
        </button>
        <button
          type="button"
          disabled={items.length === 0 || submitting}
          onClick={() => {
            setSubmitting(true);
            onConfirm(items);
          }}
          data-testid="folder-update-confirm"
          className="rounded-md bg-acc px-3 py-1.5 font-medium text-bg-1 hover:opacity-90 disabled:opacity-40"
          style={{ fontSize: "11.5px" }}
        >
          {submitting ? "Updating…" : `Update ${items.length} skill${items.length === 1 ? "" : "s"}`}
        </button>
      </div>
    </div>
  );
}

function DeleteSkillConfirm({
  skill,
  referents,
  path,
  onCancel,
  onConfirm,
}: {
  skill: Skill;
  referents: SkillReferents;
  path: string;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  const count =
    Number(referents.instance) + referents.projects.length + referents.pipelines.length + referents.runs.length;
  return (
    <div className="flex min-h-0 flex-1 flex-col p-5" data-testid="skill-delete">
      <h3 className="font-semibold text-fg" style={{ fontSize: "17px" }}>
        Delete {skill.name}?
      </h3>
      <p className="mt-2 text-fg-3" style={{ fontSize: "12px" }}>
        {count === 0 ? (
          "No live reference. Nothing else will change."
        ) : (
          <>
            These <strong className="text-fg">{count} live reference{count === 1 ? "" : "s"}</strong> will keep the id
            and show a warning; their runs still start.
          </>
        )}
      </p>
      <div
        className="mt-3 rounded-md border border-line bg-bg-3 px-3 py-2.5 font-mono text-fg-3"
        style={{ fontSize: "10.5px" }}
        data-testid="skill-referents"
      >
        {count === 0 && <div>No live references.</div>}
        {referents.instance && <ReferentLine tier="INSTANCE" label="settings" />}
        {referents.projects.map((item) => <ReferentLine key={`p-${item.id}`} tier="PROJECT" label={item.name} />)}
        {referents.pipelines.map((item, i) => (
          <ReferentLine key={`l-${item.id}-${item.node_id ?? i}`} tier="PIPELINE" label={`${item.name}${item.node_id ? ` · node ${item.node_id}` : ""}`} />
        ))}
        {referents.runs.map((item) => (
          <ReferentLine key={`r-${item.run_id}`} tier="RUN" label={`${item.name ?? item.run_id} (not started)`} />
        ))}
      </div>
      <p className="mt-3 rounded-md border border-st-blocked/40 bg-st-blocked-bg px-3 py-2 text-fg-2" style={{ fontSize: "11px" }}>
        Runs already started are untouched: their skill content was frozen at spawn. The folder{" "}
        <span className="font-mono">{path}/</span> is removed.
      </p>
      <div className="mt-4 flex justify-end gap-2">
        <button
          type="button"
          onClick={onCancel}
          className="rounded-md border border-line-strong bg-bg-3 px-3 py-1.5 text-fg-2 hover:bg-bg-4"
          style={{ fontSize: "11.5px" }}
        >
          Cancel
        </button>
        <button
          type="button"
          onClick={onConfirm}
          data-testid="skill-delete-confirm"
          className="rounded-md bg-st-failed px-3 py-1.5 font-medium text-white hover:opacity-90"
          style={{ fontSize: "11.5px" }}
        >
          {count === 0 ? "Delete" : "Delete anyway"}
        </button>
      </div>
    </div>
  );
}

function ReferentLine({ tier, label }: { tier: string; label: string }) {
  return (
    <div className="flex gap-4">
      <span className="w-[72px] shrink-0 text-fg-4">{tier}</span>
      <span className="text-fg-2">{label}</span>
    </div>
  );
}

function DeleteFolderConfirm({
  folder,
  count,
  parentName,
  onCancel,
  onConfirm,
}: {
  folder: SkillFolder;
  count: number;
  parentName: string | null;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  return (
    <div className="flex min-h-0 flex-1 flex-col p-5" data-testid="folder-delete">
      <h3 className="font-semibold text-fg" style={{ fontSize: "17px" }}>
        Delete folder {folder.name}?
      </h3>
      <p className="mt-2 text-fg-3" style={{ fontSize: "12px" }}>
        {count === 0
          ? "The folder is empty."
          : `Its ${count} skill${count === 1 ? "" : "s"} and sub-folders move to ${parentName ? `“${parentName}”` : "the root of the bank"}. No skill is deleted.`}
        {folder.source && (
          <>
            {" "}
            The link to <span className="font-mono">{displaySourceUrl(folder.source.url)}</span> is lost with the folder; the
            skills keep their own provenance.
          </>
        )}
      </p>
      <div className="mt-4 flex justify-end gap-2">
        <button
          type="button"
          onClick={onCancel}
          className="rounded-md border border-line-strong bg-bg-3 px-3 py-1.5 text-fg-2 hover:bg-bg-4"
          style={{ fontSize: "11.5px" }}
        >
          Cancel
        </button>
        <button
          type="button"
          onClick={onConfirm}
          data-testid="folder-delete-confirm"
          className="rounded-md bg-st-failed px-3 py-1.5 font-medium text-white hover:opacity-90"
          style={{ fontSize: "11.5px" }}
        >
          Delete folder
        </button>
      </div>
    </div>
  );
}
