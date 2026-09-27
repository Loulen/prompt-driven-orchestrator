import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { ApiError } from "../../shared/http/client";
import { createSkill } from "../api/bank";
import { uploadSkillFileFromPath, uploadSkillFiles } from "../api/files";
import type { Skill } from "../../shared/generated/Skill";
import { validateSkillMd, type CheckId, type CheckState } from "../model/skillMd";
import { mergeStaged, sortDroppedFiles, sortHostPicks, type RefusedItem, type StagedFile } from "../model/skillFiles";
import { useFileDropTarget } from "./useFileDropTarget";

/** Map a daemon refusal `code` to the check it should light red. */
const CODE_TO_CHECK: Record<string, CheckId> = {
  no_frontmatter: "frontmatter",
  malformed_frontmatter: "frontmatter",
  missing_name: "name",
  name_not_kebab_case: "name",
  empty_label: "name",
  missing_description: "description",
  empty_body: "body",
  duplicate_name: "unique",
};

type Phase = "edit" | "uploading" | "settle";

/**
 * The state of "New skill from SKILL.md" (#668, #671): the pasted text and its
 * five checks, the reference files staged locally, then the create + uploads.
 * The skill exists as soon as it is created: an upload failure settles the
 * popup (Retry / Skip per row) instead of closing it.
 */
export function usePasteSkill({
  existingNames,
  initialFolderId,
  onClose,
  onCreated,
}: {
  existingNames: string[];
  initialFolderId: string | null;
  onClose: () => void;
  onCreated: (skill: Skill, fileCount: number) => void | Promise<void>;
}) {
  const [text, setText] = useState("");
  const [folderId, setFolderId] = useState<string | null>(initialFolderId);
  const [submitting, setSubmitting] = useState(false);
  const [serverError, setServerError] = useState<{ code: string; message: string } | null>(null);
  /** The text a 409 was returned for; the `unique` check stays red until it changes. */
  const [duplicateFor, setDuplicateFor] = useState<string | null>(null);
  const [confirmDiscard, setConfirmDiscard] = useState(false);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  // Reference files (#671).
  const [staged, setStaged] = useState<StagedFile[]>([]);
  const [refused, setRefused] = useState<RefusedItem[]>([]);
  /** The text a dropped SKILL.md replaced; ⌘Z restores it while set. */
  const [replacedFrom, setReplacedFrom] = useState<string | null>(null);
  const [explorerOpen, setExplorerOpen] = useState(false);
  const [phase, setPhase] = useState<Phase>("edit");
  const [created, setCreated] = useState<Skill | null>(null);

  useEffect(() => {
    textareaRef.current?.focus();
  }, []);

  const serverDuplicate = duplicateFor !== null && duplicateFor === text;
  const validation = useMemo(
    () => validateSkillMd(text, existingNames, serverDuplicate),
    [text, existingNames, serverDuplicate],
  );

  // A daemon 400 for the CURRENT text overrides the check it names; any edit clears it.
  const serverCheck: CheckId | null =
    serverError && CODE_TO_CHECK[serverError.code] ? CODE_TO_CHECK[serverError.code] : null;
  const checks = validation.checks.map((check) =>
    serverCheck === check.id && check.state === "pass"
      ? { ...check, state: "fail" as CheckState }
      : check,
  );
  const failing = checks.some((check) => check.state === "fail");
  const canCreate = validation.valid && !serverError && !submitting && text.trim() !== "";
  const reason = serverError?.message ?? validation.reason;
  const editing = phase === "edit";
  const busy = phase === "uploading";

  const finish = async () => {
    if (!created) return;
    const uploadedCount = staged.filter((file) => file.status.state === "uploaded").length;
    await onCreated(created, uploadedCount);
    onClose();
  };

  const requestClose = () => {
    if (busy) return;
    if (phase === "settle") {
      void finish();
      return;
    }
    if ((text.trim() !== "" || staged.length > 0) && !confirmDiscard) {
      setConfirmDiscard(true);
      return;
    }
    onClose();
  };

  // A layout effect: the listener is re-bound in the same commit that shows the
  // new state, so a ⌘Z right after a dropped SKILL.md replaced the text always
  // sees `replacedFrom` (a passive effect could still hold the old closure).
  useLayoutEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        // The explorer owns Escape while it is stacked on top.
        if (explorerOpen) return;
        event.stopPropagation();
        requestClose();
      }
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "z" && replacedFrom !== null && editing) {
        event.preventDefault();
        event.stopPropagation();
        setText(replacedFrom);
        setReplacedFrom(null);
        setServerError(null);
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [text, confirmDiscard, staged.length, phase, replacedFrom, explorerOpen]);

  // ---- files -------------------------------------------------------------

  const replaceTextWith = useCallback(
    (next: string) => {
      setReplacedFrom((prev) => prev ?? text);
      setText(next);
      setServerError(null);
      setDuplicateFor(null);
      setConfirmDiscard(false);
    },
    [text],
  );

  const acceptFiles = useCallback(
    (files: ArrayLike<File>, items?: ArrayLike<DataTransferItem> | null) => {
      if (!editing) return;
      const sorted = sortDroppedFiles(files, items);
      if (sorted.files.length > 0) setStaged((prev) => mergeStaged(prev, sorted.files));
      setRefused(sorted.refused);
      setConfirmDiscard(false);
      if (sorted.skillMd) {
        void sorted.skillMd.text().then((content) => replaceTextWith(content));
      }
    },
    [editing, replaceTextWith],
  );

  const onDrop = useCallback(
    (dataTransfer: DataTransfer) => acceptFiles(dataTransfer.files, dataTransfer.items),
    [acceptFiles],
  );
  const { dragging, handlers: dropHandlers } = useFileDropTarget(onDrop);

  const acceptHostPicks = (paths: string[]) => {
    const picks = sortHostPicks(paths);
    if (picks.files.length > 0) setStaged((prev) => mergeStaged(prev, picks.files));
    setRefused(
      picks.skillMdPath
        ? [{ name: "SKILL.md", reason: "Paste its text above, or drop it from your desktop" }]
        : [],
    );
  };

  const removeStaged = (path: string) => setStaged((prev) => prev.filter((file) => file.path !== path));

  // ---- create + upload ----------------------------------------------------

  const setStatus = (path: string, status: StagedFile["status"]) =>
    setStaged((prev) => prev.map((file) => (file.path === path ? { ...file, status } : file)));

  const uploadOne = async (skill: Skill, file: StagedFile): Promise<boolean> => {
    setStatus(file.path, { state: "uploading" });
    try {
      const result =
        file.source.kind === "browser"
          ? await uploadSkillFiles(skill.id, [{ path: file.path, file: file.source.file }])
          : await uploadSkillFileFromPath(skill.id, file.source.fromPath, file.path);
      const landed = result.uploaded.find((entry) => entry.path === file.path) ?? result.uploaded[0];
      setStatus(file.path, { state: "uploaded", size: landed?.size ?? file.size ?? 0 });
      return true;
    } catch (cause) {
      const status = cause instanceof ApiError && cause.status ? `${cause.status} · ` : "";
      setStatus(file.path, {
        state: "failed",
        message: `${status}${cause instanceof Error ? cause.message : "upload failed"}`,
      });
      return false;
    }
  };

  const submit = async () => {
    if (!canCreate) return;
    setSubmitting(true);
    setServerError(null);
    let skill: Skill;
    try {
      skill = await createSkill({ content: text, folder_id: folderId });
    } catch (cause) {
      if (cause instanceof ApiError) {
        const body = cause.body as { code?: unknown } | null;
        const code = typeof body?.code === "string" ? body.code : "";
        if (cause.status === 409 || code === "duplicate_name") {
          setDuplicateFor(text);
        } else {
          setServerError({ code, message: cause.message });
        }
      } else {
        setServerError({
          code: "",
          message: cause instanceof Error ? cause.message : "Failed to create the skill",
        });
      }
      setSubmitting(false);
      return;
    }
    setCreated(skill);
    if (staged.length === 0) {
      await onCreated(skill, 0);
      onClose();
      return;
    }
    // The skill exists from here on: the popup never closes on a failure.
    setPhase("uploading");
    let allGood = true;
    for (const file of staged) {
      const ok = await uploadOne(skill, file);
      if (!ok) allGood = false;
    }
    setSubmitting(false);
    if (allGood) {
      await onCreated(skill, staged.length);
      onClose();
    } else {
      setPhase("settle");
    }
  };

  const retry = async (file: StagedFile) => {
    if (!created) return;
    setPhase("uploading");
    await uploadOne(created, file);
    setPhase("settle");
  };
  const skip = (file: StagedFile) => setStatus(file.path, { state: "skipped" });

  return {
    text, setText, folderId, setFolderId, submitting, confirmDiscard, setConfirmDiscard, textareaRef,
    staged, refused, setRefused, replacedFrom, setReplacedFrom, explorerOpen, setExplorerOpen, phase,
    setServerError, validation, checks, failing, canCreate, reason, editing, busy,
    finish, requestClose, acceptFiles, acceptHostPicks, removeStaged, dragging, dropHandlers,
    submit, retry, skip,
  };
}

export type PasteSkillState = ReturnType<typeof usePasteSkill>;
