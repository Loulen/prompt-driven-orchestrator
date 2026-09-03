import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  ClipboardPaste,
  Copy,
  FileText,
  Folder,
  FolderInput,
  FolderPlus,
  MoreVertical,
  Pencil,
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
  updateSkill,
  updateSkillFolder,
} from "../api";
import type { Skill, SkillBank, SkillDetail, SkillFolder, SkillReferents } from "../types";
import { timeAgo } from "../lib/skillMd";
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
import SkillFilesTab from "./SkillFilesTab";
import { useSkillFiles } from "../hooks/useSkillFiles";
import { useFileDropTarget } from "../hooks/useFileDropTarget";
import { DropOverlay } from "./SkillFileDropZone";

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
  | { kind: "delete-folder"; folder: SkillFolder };

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
  /** Bumped after every file write so the detail (its `files`) is re-read (#671). */
  const [detailVersion, setDetailVersion] = useState(0);
  /** The editor holds an unsaved draft: leaving the skill or the tab asks first. */
  const [filesDirty, setFilesDirty] = useState(false);
  const [leaveRequest, setLeaveRequest] = useState<(() => void) | null>(null);

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
  }, [selectedSkillId, selectedSkillVersion, detailVersion]);

  const refreshDetail = useCallback(() => setDetailVersion((v) => v + 1), []);

  /**
   * Run `action` now, or — when the file editor is dirty — hand it to the
   * editor header, which asks Save / Discard / Stay and runs it on the first two
   * (#671 design 07: "changer de fichier ou de skill … demande d'abord").
   */
  const guarded = (action: () => void) => {
    if (filesDirty) {
      setLeaveRequest(() => action);
      return;
    }
    action();
  };

  // Close a kebab on outside click / Escape.
  useEffect(() => {
    if (!menuFor && !movePickerFor) return;
    const close = () => {
      setMenuFor(null);
      setMovePickerFor(null);
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
  }, [menuFor, movePickerFor]);

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
          <button
            type="button"
            onClick={() => setPasteOpen(true)}
            data-testid="skill-paste"
            className="flex shrink-0 items-center gap-1.5 rounded-md bg-acc px-2.5 py-1.5 font-medium text-bg-1 hover:opacity-90"
            style={{ fontSize: "11.5px" }}
          >
            <ClipboardPaste size={12} />
            Paste skill
          </button>
        </div>

        <div className="flex min-h-0 flex-1 flex-col overflow-y-auto">
          <SkillTree
            folders={folders}
            skills={skills}
            selected={selectedRef}
            onSelect={(ref) =>
              guarded(() => {
                setSelected(ref);
                setPending(null);
                setMenuFor(null);
              })
            }
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
        ) : selectedSkill ? (
          <SkillDetailView
            skill={selectedSkill}
            detail={detail}
            folders={folders}
            tab={detailTab}
            onTab={(tab) => guarded(() => setDetailTab(tab))}
            existingNames={existingNames}
            refreshDetail={refreshDetail}
            onSkillChanged={onChanged}
            showToast={(message) => showToast({ message })}
            onError={setError}
            pathLabel={bank.root_path ? `${relativise(bank.root_path, home)}/${shortId(selectedSkill.id)}/` : null}
            leaveRequest={leaveRequest}
            onLeaveSettled={() => setLeaveRequest(null)}
            onDirtyChange={setFilesDirty}
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
          />
        ) : selectedFolder ? (
          <FolderDetailView
            folder={selectedFolder}
            folders={folders}
            count={counts.get(selectedFolder.id) ?? 0}
            onNewSubfolder={() => void newFolder(selectedFolder.id)}
            onRename={() => startRename({ kind: "folder", id: selectedFolder.id })}
            onDelete={() => void askDelete({ kind: "folder", id: selectedFolder.id })}
            onMoveTo={(parentId) => void moveFolder(selectedFolder.id, parentId)}
          />
        ) : emptyBank ? (
          <EmptyBank onPaste={() => setPasteOpen(true)} />
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

      {pasteOpen && (
        <PasteSkillModal
          folders={folders}
          existingNames={existingNames}
          initialFolderId={selectedFolder?.id ?? selectedSkill?.folder_id ?? null}
          onClose={() => setPasteOpen(false)}
          onCreated={async (skill, fileCount) => {
            await onChanged();
            expandTo(skill.folder_id ?? null);
            setSelected({ kind: "skill", id: skill.id });
            // Files attached: land on the Files tab, so FP #671 step 2 reads
            // without a click.
            setDetailTab(fileCount > 0 ? "files" : "skill");
          }}
        />
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------

function MenuItem({
  icon,
  label,
  hint,
  danger,
  onClick,
}: {
  icon: React.ReactNode;
  label: string;
  hint?: string;
  danger?: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      role="menuitem"
      onClick={onClick}
      className={`flex w-full items-center gap-2 rounded px-2 py-1.5 text-left hover:bg-bg-5 ${
        danger ? "text-st-failed" : "text-fg-2"
      }`}
      style={{ fontSize: "11.5px" }}
    >
      <span className="shrink-0">{icon}</span>
      <span className="flex-1">{label}</span>
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

function EmptyBank({ onPaste }: { onPaste: () => void }) {
  return (
    <div className="flex flex-1 flex-col items-center justify-center gap-3 p-8 text-center" data-testid="skill-bank-empty">
      <div className="grid h-14 w-14 place-items-center rounded-xl border border-dashed border-line-strong text-fg-4">
        <FileText size={20} />
      </div>
      <h3 className="font-semibold text-fg" style={{ fontSize: "14px" }}>
        No skills yet
      </h3>
      <p className="max-w-[380px] text-fg-3" style={{ fontSize: "12px" }}>
        Paste the text of a <span className="font-mono">SKILL.md</span> to add one. Skills are
        delivered to every worktree PDO creates, never committed.
      </p>
      <button
        type="button"
        onClick={onPaste}
        data-testid="skill-paste-empty"
        className="mt-1 flex items-center gap-1.5 rounded-md bg-acc px-3 py-2 font-medium text-bg-1 hover:opacity-90"
        style={{ fontSize: "12px" }}
      >
        <ClipboardPaste size={13} />
        Paste skill
      </button>
      <p className="text-fg-4" style={{ fontSize: "11px" }}>
        Import from a repo or a local folder arrives in a later ticket.
      </p>
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
      className={`flex items-center gap-1.5 rounded-md border px-2.5 py-1.5 transition-colors ${
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
  existingNames,
  refreshDetail,
  onSkillChanged,
  showToast,
  onError,
  pathLabel,
  leaveRequest,
  onLeaveSettled,
  onDirtyChange,
  copied,
  onCopyId,
  onRename,
  onMove,
  onDelete,
  movePickerOpen,
  onMoveTo,
}: {
  skill: Skill;
  detail: SkillDetail | null;
  folders: SkillFolder[];
  tab: "skill" | "files";
  onTab: (tab: "skill" | "files") => void;
  existingNames: string[];
  refreshDetail: () => void;
  onSkillChanged: () => Promise<void>;
  showToast: (message: string) => void;
  onError: (message: string) => void;
  pathLabel: string | null;
  leaveRequest: (() => void) | null;
  onLeaveSettled: () => void;
  onDirtyChange: (dirty: boolean) => void;
  copied: boolean;
  onCopyId: () => void;
  onRename: () => void;
  onMove: () => void;
  onDelete: () => void;
  movePickerOpen: boolean;
  onMoveTo: (folderId: string | null) => void;
}) {
  const current = detail && detail.id === skill.id ? detail : null;
  const fileCount = current?.files.length ?? 0;
  const files = useSkillFiles({
    skillId: skill.id,
    skillName: skill.name,
    detail: current,
    existingNames,
    refreshDetail,
    onSkillChanged,
    showToast,
    onError,
  });
  useEffect(() => {
    onDirtyChange(files.dirty);
  }, [files.dirty, onDirtyChange]);
  useEffect(() => () => onDirtyChange(false), [onDirtyChange]);
  // A file drag anywhere over the detail switches to the Files tab and covers
  // the pane with the drop overlay (#671 design 02/05).
  const { dragging, handlers: dropHandlers } = useFileDropTarget(
    (dataTransfer) => void files.acceptDrop(dataTransfer),
    () => {
      if (tab !== "files") onTab("files");
    },
  );
  const frontmatter = current?.frontmatter ?? null;
  const frontmatterKeys = frontmatter
    ? [
        ...(current?.frontmatter_keys ?? []).filter((key) => key in frontmatter),
        ...Object.keys(frontmatter).filter((key) => !(current?.frontmatter_keys ?? []).includes(key)),
      ]
    : [];
  return (
    <div className="relative flex min-h-0 flex-1 flex-col p-5" data-testid="skill-detail" {...dropHandlers}>
      {dragging !== null && (
        <DropOverlay count={dragging} hint="A SKILL.md replaces the skill text · folders are refused" />
      )}
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
        <span>created by {skill.source ? "import" : "paste"}</span>
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
        <SkillFilesTab files={files} pathLabel={pathLabel} leaveRequest={leaveRequest} onLeaveSettled={onLeaveSettled} />
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
  count,
  onNewSubfolder,
  onRename,
  onDelete,
  onMoveTo,
}: {
  folder: SkillFolder;
  folders: SkillFolder[];
  count: number;
  onNewSubfolder: () => void;
  onRename: () => void;
  onDelete: () => void;
  onMoveTo: (parentId: string | null) => void;
}) {
  const [picker, setPicker] = useState(false);
  const exclude = useMemo(() => descendantFolderIds(folder.id, folders), [folder.id, folders]);
  return (
    <div className="flex min-h-0 flex-1 flex-col p-5" data-testid="folder-detail">
      <div className="flex items-center gap-2">
        <Folder size={16} className="text-st-await" />
        <h3 className="truncate font-semibold text-fg" style={{ fontSize: "17px" }}>
          {folderPathLabel(folder.id, folders)}
        </h3>
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
  const triggers = referents.triggers ?? [];
  const count =
    Number(referents.instance) +
    referents.projects.length +
    triggers.length +
    referents.pipelines.length +
    referents.runs.length;
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
        {triggers.map((item) => <ReferentLine key={`t-${item.id}`} tier="TRIGGER" label={item.name} />)}
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
