import { useCallback, useEffect, useRef, useState } from "react";
import { ApiError, deleteSkillFile, fetchSkillFile, uploadSkillFileFromPath, uploadSkillFiles, writeSkillFile } from "../api";
import type { SkillDetail, SkillFileContent } from "../types";
import { validateSkillMd } from "../lib/skillMd";
import { SKILL_MD, sortDroppedFiles, sortHostPicks, type RefusedItem } from "../lib/skillFiles";

const JUST_ADDED_MS = 4000;

interface EditorState {
  path: string;
  content: SkillFileContent | null;
  draft: string;
  /** What the daemon holds; `draft !== saved` is the orange dot. */
  saved: string;
  loading: boolean;
  error: string | null;
  savedAt: number | null;
}

export interface UseSkillFilesArgs {
  skillId: string;
  /** The skill's own label, excluded from the `unique` check when editing its SKILL.md. */
  skillName: string;
  detail: SkillDetail | null;
  /** Bank labels, for the five checks on a SKILL.md save. */
  existingNames: string[];
  /** Re-read the detail (the list), and refresh the bank when SKILL.md changed. */
  refreshDetail: () => void;
  onSkillChanged: () => Promise<void>;
  showToast: (message: string) => void;
  onError: (message: string) => void;
}

/**
 * The reference-files state of one skill (#671): the list is the detail's
 * `files`; adding, saving and deleting **write immediately** (the bank has no
 * unsaved state except the editor's draft). A dropped `SKILL.md` replaces the
 * skill text: saved on the spot when the five checks pass, otherwise the editor
 * opens on it with the text unsaved and the reason in red, like an invalid
 * keystroke. The safety net is Revert, not a confirmation box.
 */
export function useSkillFiles({
  skillId,
  skillName,
  detail,
  existingNames,
  refreshDetail,
  onSkillChanged,
  showToast,
  onError,
}: UseSkillFilesArgs) {
  const [editor, setEditor] = useState<EditorState | null>(null);
  const [refused, setRefused] = useState<RefusedItem[]>([]);
  const [justAdded, setJustAdded] = useState<Set<string>>(() => new Set());
  const [replacedByDrop, setReplacedByDrop] = useState<string | null>(null);
  const [confirmDeleteFor, setConfirmDeleteFor] = useState<string | null>(null);
  const [explorerOpen, setExplorerOpen] = useState(false);
  const [saving, setSaving] = useState(false);
  const timers = useRef<number[]>([]);

  useEffect(
    () => () => {
      for (const timer of timers.current) window.clearTimeout(timer);
    },
    [],
  );

  // A new skill: forget the previous one's editor and flashes.
  useEffect(() => {
    setEditor(null);
    setRefused([]);
    setReplacedByDrop(null);
    setConfirmDeleteFor(null);
    setJustAdded(new Set());
  }, [skillId]);

  const flashAdded = useCallback((paths: string[]) => {
    setJustAdded((prev) => new Set([...prev, ...paths]));
    timers.current.push(
      window.setTimeout(() => {
        setJustAdded((prev) => {
          const next = new Set(prev);
          for (const path of paths) next.delete(path);
          return next;
        });
      }, JUST_ADDED_MS),
    );
  }, []);

  const dirty = editor !== null && editor.content !== null && !editor.content.binary && editor.draft !== editor.saved;

  // ---- editor --------------------------------------------------------------

  const openFile = useCallback(
    (path: string, initialDraft?: string, initialError?: string | null) => {
      setEditor({ path, content: null, draft: initialDraft ?? "", saved: "", loading: true, error: initialError ?? null, savedAt: null });
      fetchSkillFile(skillId, path)
        .then((content) => {
          setEditor((prev) =>
            prev && prev.path === path
              ? {
                  ...prev,
                  content,
                  loading: false,
                  saved: content.text ?? "",
                  draft: initialDraft !== undefined ? initialDraft : content.text ?? "",
                }
              : prev,
          );
        })
        .catch((cause) => {
          setEditor((prev) =>
            prev && prev.path === path
              ? { ...prev, loading: false, error: cause instanceof Error ? cause.message : "Failed to read the file" }
              : prev,
          );
        });
    },
    [skillId],
  );

  const closeEditor = useCallback(() => setEditor(null), []);
  const setDraft = (draft: string) => setEditor((prev) => (prev ? { ...prev, draft, error: null } : prev));
  const revert = () => setEditor((prev) => (prev ? { ...prev, draft: prev.saved, error: null } : prev));

  /** Local five checks for a SKILL.md draft; `null` when it passes. */
  const skillMdReason = (text: string): string | null => {
    const others = existingNames.filter((name) => name.toLowerCase() !== skillName.toLowerCase());
    const validation = validateSkillMd(text, others, false);
    return validation.valid ? null : validation.reason ?? "The SKILL.md does not pass the five checks.";
  };

  const save = useCallback(async (): Promise<boolean> => {
    if (!editor || !editor.content || editor.content.binary) return true;
    if (editor.draft === editor.saved) return true;
    if (editor.path === SKILL_MD) {
      const reason = skillMdReason(editor.draft);
      if (reason) {
        setEditor((prev) => (prev ? { ...prev, error: reason } : prev));
        return false;
      }
    }
    setSaving(true);
    try {
      const result = await writeSkillFile(skillId, editor.path, editor.draft);
      setEditor((prev) =>
        prev && prev.path === editor.path
          ? {
              ...prev,
              saved: prev.draft,
              savedAt: Date.now(),
              error: null,
              content: prev.content ? { ...prev.content, size: result.size, text: prev.draft } : prev.content,
            }
          : prev,
      );
      if (editor.path === SKILL_MD) await onSkillChanged();
      refreshDetail();
      return true;
    } catch (cause) {
      setEditor((prev) =>
        prev && prev.path === editor.path
          ? { ...prev, error: cause instanceof Error ? cause.message : "Save failed" }
          : prev,
      );
      return false;
    } finally {
      setSaving(false);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [editor, skillId, existingNames, skillName, onSkillChanged, refreshDetail]);

  // ---- add -----------------------------------------------------------------

  const replaceSkillMd = async (text: string) => {
    const reason = skillMdReason(text);
    if (reason) {
      openFile(SKILL_MD, text, reason);
      return;
    }
    try {
      await writeSkillFile(skillId, SKILL_MD, text);
      setReplacedByDrop(SKILL_MD);
      timers.current.push(window.setTimeout(() => setReplacedByDrop(null), 8000));
      if (editor?.path === SKILL_MD) openFile(SKILL_MD);
      await onSkillChanged();
      refreshDetail();
      showToast("SKILL.md replaced by the dropped file");
    } catch (cause) {
      const message = cause instanceof Error ? cause.message : "The dropped SKILL.md was refused";
      openFile(SKILL_MD, text, message);
    }
  };

  const acceptDrop = useCallback(
    async (dataTransfer: DataTransfer) => {
      const sorted = sortDroppedFiles(dataTransfer.files, dataTransfer.items);
      setRefused(sorted.refused);
      const added: string[] = [];
      for (const file of sorted.files) {
        if (file.source.kind !== "browser") continue;
        try {
          await uploadSkillFiles(skillId, [{ path: file.path, file: file.source.file }]);
          added.push(file.path);
        } catch (cause) {
          setRefused((prev) => [
            ...prev,
            { name: file.path, reason: cause instanceof Error ? cause.message : "upload failed" },
          ]);
        }
      }
      if (added.length > 0) {
        flashAdded(added);
        refreshDetail();
        showToast(added.length === 1 ? `Added ${added[0]}` : `Added ${added.length} files`);
      }
      if (sorted.skillMd) {
        const text = await sorted.skillMd.text();
        await replaceSkillMd(text);
      }
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [skillId, flashAdded, refreshDetail, showToast, existingNames, skillName, editor?.path],
  );

  const acceptPickedFiles = (files: FileList) => {
    const dataTransfer = { files, items: null } as unknown as DataTransfer;
    void acceptDrop(dataTransfer);
  };

  const acceptHostPicks = async (paths: string[]) => {
    const picks = sortHostPicks(paths);
    const added: string[] = [];
    const refusedNow: RefusedItem[] = [];
    for (const file of picks.files) {
      if (file.source.kind !== "host") continue;
      try {
        await uploadSkillFileFromPath(skillId, file.source.fromPath, file.path);
        added.push(file.path);
      } catch (cause) {
        refusedNow.push({ name: file.path, reason: cause instanceof Error ? cause.message : "copy failed" });
      }
    }
    if (picks.skillMdPath) {
      refusedNow.push({ name: SKILL_MD, reason: "Drop it from your desktop to replace the skill text" });
    }
    setRefused(refusedNow);
    if (added.length > 0) {
      flashAdded(added);
      refreshDetail();
      showToast(added.length === 1 ? `Added ${added[0]}` : `Added ${added.length} files`);
    }
  };

  // ---- delete ----------------------------------------------------------------

  const confirmDelete = async (path: string) => {
    setConfirmDeleteFor(null);
    try {
      await deleteSkillFile(skillId, path);
      if (editor?.path === path) setEditor(null);
      refreshDetail();
      showToast(`Deleted ${path}`);
    } catch (cause) {
      onError(
        cause instanceof ApiError && cause.status === 404
          ? `${path} was already gone; the list is refreshed.`
          : cause instanceof Error
            ? cause.message
            : "Delete failed",
      );
      refreshDetail();
    }
  };

  return {
    detail,
    editor,
    dirty,
    saving,
    refused,
    justAdded,
    replacedByDrop,
    confirmDeleteFor,
    explorerOpen,
    openFile,
    closeEditor,
    setDraft,
    revert,
    save,
    acceptDrop,
    acceptPickedFiles,
    acceptHostPicks,
    setExplorerOpen,
    setConfirmDeleteFor,
    confirmDelete,
    dismissRefused: (item: RefusedItem) => setRefused((prev) => prev.filter((other) => other !== item)),
  };
}

export type SkillFiles = ReturnType<typeof useSkillFiles>;
