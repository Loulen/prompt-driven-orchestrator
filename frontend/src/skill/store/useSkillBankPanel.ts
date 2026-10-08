import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ApiError } from "../../shared/http/client";
import {
  createSkillFolder,
  deleteSkill,
  deleteSkillFolder,
  fetchSkill,
  fetchSkillReferents,
  updateSkill,
  updateSkillFolder,
} from "../api/bank";
import { rescanSkillFolder, updateSkillFolderFromSource } from "../api/import";
import type { Skill } from "../../shared/generated/Skill";
import type { SkillBank, SkillDetail, SkillReferents } from "../model/skill";
import type { SkillFolder } from "../../shared/generated/SkillFolder";
import type { SkillRescanReport } from "../../shared/generated/SkillRescanReport";
import { displaySourceUrl } from "../model/skillSource";
import { folderCounts, type TreeNodeRef } from "../model/skillTree";
import { newScanId } from "../model/scanId";
import { tildify } from "../../lib/homePath";
import { copyToClipboard } from "../../shared/lib/clipboard";

const UNDO_MS = 6000;

export type Pending =
  | { kind: "delete-skill"; skill: Skill; referents: SkillReferents }
  | { kind: "delete-folder"; folder: SkillFolder }
  /** Update from source (#670): the re-scan is running, then its diff awaits confirmation. */
  | { kind: "rescan-folder"; folder: SkillFolder }
  | { kind: "update-folder"; folder: SkillFolder; report: SkillRescanReport };

export interface Toast {
  message: string;
  undo?: () => Promise<void>;
}

/** The seeded skill (#722): locked against rename, move, edit and delete. */
export function isLocked(skill: Pick<Skill, "locked"> | null | undefined): boolean {
  return skill?.locked ?? false;
}

/**
 * The state and the gestures of the Banque de skills drill-down (#668). Every
 * gesture commits immediately (POST/PUT/DELETE) and calls `onChanged`, so
 * Back / Esc / close never lose anything. Move and rename are undoable by
 * toast (6 s); create and delete are not.
 */
export function useSkillBankPanel(bank: SkillBank, home: string | null, onChanged: () => Promise<void>) {
  const { skills, folders } = bank;
  const [selected, setSelected] = useState<TreeNodeRef | null>(null);
  const [expanded, setExpanded] = useState<Set<string>>(() => new Set());
  const [filter, setFilter] = useState("");
  const [pasteOpen, setPasteOpen] = useState(false);
  const [importOpen, setImportOpen] = useState(false);
  const [addMenuOpen, setAddMenuOpen] = useState(false);
  const [detail, setDetail] = useState<SkillDetail | null>(null);
  const [detailTab, setDetailTab] = useState<string>("skill");
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
    if (isLocked(skill)) return;
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
    if (ref.kind === "skill" && isLocked(skillById.get(ref.id))) return;
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
    if (ref.kind === "skill" && isLocked(skillById.get(ref.id))) return;
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
    // Clipboard unavailable: nothing to do.
    if (!(await copyToClipboard(id))) return;
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1500);
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
            : `Updated ${n} skill${n === 1 ? "" : "s"} from ${tildify(displaySourceUrl(folder.source?.url ?? "the source"), home)}`,
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

  const toggleExpanded = (id: string) =>
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  return {
    selected, setSelected, selectedRef, selectedSkill, selectedFolder,
    expanded, toggleExpanded, expandTo, filter, setFilter,
    pasteOpen, setPasteOpen, importOpen, setImportOpen, addMenuOpen, setAddMenuOpen,
    detail, detailTab, setDetailTab, refreshDetail,
    renaming, setRenaming, renameError, setRenameError,
    pending, setPending, menuFor, setMenuFor, movePickerFor, setMovePickerFor,
    toast, setToast, toastTimer, showToast, error, setError, failWith, copied,
    leaveRequest, setLeaveRequest, setFilesDirty, guarded,
    folderById, counts, existingNames,
    moveSkill, moveFolder, commitRename, startRename, newFolder, askDelete, confirmDelete, copyId,
    startUpdateFromSource, confirmUpdateFromSource, openSourceFolder,
  };
}

export type SkillBankPanelState = ReturnType<typeof useSkillBankPanel>;
