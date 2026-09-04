import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Check, ClipboardPaste, FileText, Folder, X } from "lucide-react";
import { ApiError, createSkill, uploadSkillFileFromPath, uploadSkillFiles } from "../api";
import type { Skill, SkillFolder } from "../types";
import { validateSkillMd, type CheckId, type CheckState } from "../lib/skillMd";
import { folderPathLabel } from "../lib/skillTree";
import {
  formatBytes,
  mergeStaged,
  sortDroppedFiles,
  sortHostPicks,
  totalStagedBytes,
  type RefusedItem,
  type StagedFile,
} from "../lib/skillFiles";
import FsExplorerModal from "./FsExplorerModal";
import SkillFileDropZone, { DropOverlay } from "./SkillFileDropZone";
import { useFileDropTarget } from "../hooks/useFileDropTarget";

interface Props {
  folders: SkillFolder[];
  /** Current bank labels, for the live `unique` check. */
  existingNames: string[];
  /** Folder pre-selected when the popup opened (the tree's selection). */
  initialFolderId: string | null;
  onClose: () => void;
  /**
   * The skill exists (and its files, if any, were uploaded or skipped).
   * `fileCount` is what landed, so the caller can open the Files tab (FP #671
   * step 2 reads without a click).
   */
  onCreated: (skill: Skill, fileCount: number) => void | Promise<void>;
}

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
 * "New skill from SKILL.md" (#668, FP steps 2-3): paste the text, five checks
 * re-run on every keystroke, the preview card is exactly the tree row it will
 * become, and Create enables only when all five pass. A refusal — local or a
 * daemon 400/409 — is shown **in place** (red check, red textarea border, callout
 * with reason and consequence), never as a modal on top of this modal.
 *
 * Reference files (#671): dropped anywhere on the popup or picked with Browse…,
 * they are **staged locally** (badge "to upload"); nothing touches disk until
 * Create, which writes the skill first and then uploads them one by one. A
 * dropped `SKILL.md` never becomes a row: it **replaces the text** (⌘Z undoes).
 * If an upload fails after the skill exists, the popup stays open in a settled
 * state — text and checks frozen, Retry / Skip per row, Done — and never lets
 * the user believe the skill was not created.
 *
 * Same `z-[60]` layer as `FsExplorerModal`'s default; Browse… opens the explorer
 * at `z-[70]` on top, and Escape is left to the explorer while it is open.
 */
export default function PasteSkillModal({
  folders,
  existingNames,
  initialFolderId,
  onClose,
  onCreated,
}: Props) {
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

  useEffect(() => {
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

  // ---- rendering ----------------------------------------------------------

  const preview = validation.parsed;
  const previewName = preview.name ?? "(name)";
  const previewDescription = preview.description ?? "(description)";
  const selectedFolder = folders.find((folder) => folder.id === folderId) ?? null;
  const failedCount = staged.filter((file) => file.status.state === "failed").length;
  const uploadingIndex = staged.findIndex((file) => file.status.state === "uploading");

  return (
    <div
      className="fixed inset-0 z-[60] flex items-center justify-center bg-black/50"
      onClick={(event) => {
        event.stopPropagation();
        requestClose();
      }}
      data-testid="paste-skill-backdrop"
    >
      <div
        className={`relative flex w-[1020px] max-w-[96vw] max-h-[88vh] flex-col rounded-lg border bg-bg-4 shadow-xl ${
          dragging !== null ? "border-acc" : "border-line"
        }`}
        onClick={(event) => event.stopPropagation()}
        role="dialog"
        aria-label="New skill from SKILL.md"
        aria-busy={busy || undefined}
        data-testid="paste-skill-modal"
        {...(editing ? dropHandlers : {})}
      >
        {dragging !== null && editing && (
          <DropOverlay count={dragging} hint="A SKILL.md replaces the skill text · folders are refused" />
        )}
        <div className="flex items-center justify-between border-b border-line px-4 py-3">
          <h3 className="flex items-center gap-2 font-semibold text-fg" style={{ fontSize: "13.5px" }}>
            <ClipboardPaste size={14} className="text-fg-3" />
            New skill from SKILL.md
          </h3>
          <button
            type="button"
            onClick={requestClose}
            disabled={busy}
            aria-label="Close paste popup"
            className="grid h-6 w-6 place-items-center rounded text-fg-3 hover:bg-bg-5 hover:text-fg disabled:opacity-30"
          >
            <X size={14} />
          </button>
        </div>

        <div className="flex min-h-0 flex-1 gap-4 overflow-y-auto p-4">
          {/* Left: the pasted text + files */}
          <div className="flex min-w-0 flex-1 flex-col gap-2">
            <div className="relative flex min-h-[240px] flex-1 flex-col">
              <textarea
                ref={textareaRef}
                value={text}
                readOnly={!editing}
                onChange={(event) => {
                  setText(event.target.value);
                  setServerError(null);
                  setConfirmDiscard(false);
                  setReplacedFrom(null);
                }}
                onPaste={() => setConfirmDiscard(false)}
                spellCheck={false}
                placeholder={"---\nname: my-skill\ndescription: What it does, when to use it.\n---\n\n# My skill\n\nInstructions…"}
                aria-label="SKILL.md text"
                aria-invalid={failing || undefined}
                data-testid="paste-skill-text"
                className={`min-h-[240px] flex-1 resize-none rounded-md border bg-bg-1 p-3 font-mono text-fg outline-none transition-colors ${
                  !editing
                    ? "border-line opacity-60"
                    : failing
                      ? "border-st-failed"
                      : text.trim() && validation.valid
                        ? "border-acc/60"
                        : "border-line-strong focus:border-acc"
                }`}
                style={{ fontSize: "11.5px", lineHeight: 1.55 }}
              />
              {replacedFrom !== null && editing && (
                <div
                  className="absolute right-3 top-3 flex items-center gap-2 rounded-md border border-acc/40 bg-bg-3 px-2.5 py-1 text-fg-2 shadow"
                  style={{ fontSize: "10.5px" }}
                  role="status"
                  data-testid="paste-skill-replaced"
                >
                  Replaced by dropped SKILL.md ·{" "}
                  <button
                    type="button"
                    className="font-medium text-acc hover:underline"
                    onClick={() => {
                      setText(replacedFrom);
                      setReplacedFrom(null);
                      setServerError(null);
                    }}
                    data-testid="paste-skill-undo-replace"
                  >
                    ⌘Z to undo
                  </button>
                </div>
              )}
            </div>

            <SkillFileDropZone
              disabled={!editing}
              testId="paste-skill-drop"
              onBrowse={() => setExplorerOpen(true)}
              onPickFiles={(files) => acceptFiles(files)}
              label={
                staged.length === 0 ? (
                  <>
                    <strong className="text-fg-2">Files</strong> · drop files anywhere in this window (a SKILL.md replaces
                    the text above), or
                  </>
                ) : (
                  <>
                    <strong className="text-fg-2">Files · {staged.length}</strong>
                    {editing ? " · drop more anywhere, or" : ""}
                  </>
                )
              }
            />

            {(staged.length > 0 || refused.length > 0) && (
              <ul className="flex flex-col gap-1" data-testid="paste-skill-files">
                {staged.map((file) => (
                  <StagedRow
                    key={file.path}
                    file={file}
                    editing={editing}
                    onRemove={() => removeStaged(file.path)}
                    onRetry={() => void retry(file)}
                    onSkip={() => skip(file)}
                  />
                ))}
                {refused.map((item) => (
                  <li
                    key={`refused-${item.name}`}
                    className="flex items-center gap-2 rounded-md border border-st-failed/60 bg-st-failed-bg px-3 py-1.5"
                    style={{ fontSize: "11px" }}
                    data-testid="paste-skill-refused"
                  >
                    <Folder size={11} className="shrink-0 text-st-failed" />
                    <span className="font-mono text-fg">{item.name}</span>
                    <span className="rounded border border-st-failed/60 px-1.5 text-st-failed" style={{ fontSize: "9.5px" }}>
                      refused
                    </span>
                    <span className="text-fg-2">{item.reason}</span>
                    <span className="flex-1" />
                    <button
                      type="button"
                      aria-label={`Dismiss ${item.name}`}
                      onClick={() => setRefused((prev) => prev.filter((other) => other !== item))}
                      className="grid h-4 w-4 place-items-center rounded text-fg-4 hover:text-fg"
                    >
                      <X size={10} />
                    </button>
                  </li>
                ))}
              </ul>
            )}
            {staged.length > 0 && editing && (
              <p className="text-fg-4" style={{ fontSize: "10.5px" }}>
                {staged.length} file{staged.length === 1 ? "" : "s"} · {formatBytes(totalStagedBytes(staged))} · uploaded after
                Create, in this order
              </p>
            )}
            {staged.length === 0 && editing && (
              <p className="text-fg-4" style={{ fontSize: "10.5px" }}>
                Files are stored next to SKILL.md and travel with the skill. You can edit them as plain text once the
                skill exists.
              </p>
            )}
            {phase === "settle" && failedCount > 0 && (
              <div
                role="alert"
                className="rounded-md border border-st-failed/50 bg-st-failed-bg px-3 py-2 text-fg-2"
                style={{ fontSize: "11px" }}
                data-testid="paste-skill-upload-failed"
              >
                <strong className="text-fg">Skill created.</strong> {failedCount} of {staged.length} file
                {staged.length === 1 ? "" : "s"} did not upload. Retry, or skip it and add it later from the skill’s
                Files tab.
              </div>
            )}
          </div>

          {/* Right: checks, preview, folder */}
          <div className={`flex w-[330px] shrink-0 flex-col gap-4 overflow-y-auto ${editing ? "" : "opacity-60"}`}>
            <section>
              <h4 className="mb-2 text-fg-4 uppercase tracking-wide" style={{ fontSize: "10px" }}>
                Checks
              </h4>
              {editing ? (
                <ul className="flex flex-col gap-1.5" data-testid="paste-skill-checks">
                  {checks.map((check) => (
                    <li
                      key={check.id}
                      className="flex items-center gap-2 text-fg-2"
                      style={{ fontSize: "11.5px" }}
                      data-testid={`check-${check.id}`}
                      data-state={check.state}
                    >
                      <CheckDot state={check.state} />
                      <span className={check.state === "fail" ? "text-fg" : check.state === "pending" ? "text-fg-4" : ""}>
                        {check.label}
                      </span>
                    </li>
                  ))}
                </ul>
              ) : (
                <div className="flex items-center gap-2 text-fg-3" style={{ fontSize: "11.5px" }}>
                  <CheckDot state="pass" /> All five pass
                </div>
              )}
            </section>

            <section>
              <h4 className="mb-2 text-fg-4 uppercase tracking-wide" style={{ fontSize: "10px" }}>
                Will appear as
              </h4>
              <div
                className={`rounded-md border border-line bg-bg-3 px-3 py-2.5 ${validation.valid ? "" : "opacity-70"}`}
                data-testid="paste-skill-preview"
              >
                <div className={`font-semibold ${preview.name ? "text-fg" : "text-fg-4"}`} style={{ fontSize: "12.5px" }}>
                  {previewName}
                </div>
                <div className={`mt-0.5 ${preview.description ? "text-fg-3" : "text-fg-4"}`} style={{ fontSize: "11px" }}>
                  {previewDescription}
                </div>
                {staged.length > 0 && (
                  <div className="mt-1.5 text-fg-4" style={{ fontSize: "10.5px" }}>
                    + {staged.length} file{staged.length === 1 ? "" : "s"}
                  </div>
                )}
              </div>
              {reason && editing && (
                <div
                  role="alert"
                  className="mt-2 rounded-md border border-st-failed/50 bg-st-failed-bg px-3 py-2 text-fg-2"
                  style={{ fontSize: "11px" }}
                  data-testid="paste-skill-reason"
                >
                  <strong className="text-fg">
                    {checks.find((c) => c.id === "unique")?.state === "fail" && !failingOther(checks)
                      ? "Name already taken."
                      : "Cannot create."}
                  </strong>{" "}
                  {reason}
                </div>
              )}
            </section>

            <section>
              <h4 className="mb-2 text-fg-4 uppercase tracking-wide" style={{ fontSize: "10px" }}>
                Folder
              </h4>
              <label className="flex items-center gap-2 rounded-md border border-line-strong bg-bg-3 px-2.5 py-1.5">
                <Folder size={12} className="shrink-0 text-fg-3" />
                <select
                  value={folderId ?? ""}
                  disabled={!editing}
                  onChange={(event) => setFolderId(event.target.value || null)}
                  aria-label="Folder"
                  data-testid="paste-skill-folder"
                  className="w-full bg-transparent text-fg outline-none"
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
              {selectedFolder === null && folderId !== null && (
                <p className="mt-1 text-st-blocked" style={{ fontSize: "10px" }}>
                  That folder no longer exists; the skill will land at the root.
                </p>
              )}
            </section>
          </div>
        </div>

        <div className="flex items-center justify-between gap-3 border-t border-line px-4 py-3">
          <span className="text-fg-4" style={{ fontSize: "10.5px" }} data-testid="paste-skill-footer">
            {editing
              ? "Validated as you type · nothing touches disk until Create"
              : busy
                ? `Uploading ${Math.min(uploadingIndex + 1, staged.length)} / ${staged.length} · the skill already exists in the bank`
                : "The skill already exists in the bank"}
          </span>
          <div className="flex items-center gap-2">
            {!editing ? (
              <button
                type="button"
                onClick={() => void finish()}
                disabled={busy}
                data-testid="paste-skill-done"
                className="rounded-md bg-acc px-3 py-1.5 font-medium text-bg-1 transition-opacity hover:opacity-90 disabled:opacity-40"
                style={{ fontSize: "11.5px" }}
              >
                Done
              </button>
            ) : confirmDiscard ? (
              <>
                <span className="text-fg-2" style={{ fontSize: "11px" }} data-testid="paste-skill-discard-prompt">
                  {text.trim() !== "" ? "Discard the pasted text" : "Discard the attached files"}
                  {text.trim() !== "" && staged.length > 0 ? " and files" : ""}?
                </span>
                <button
                  type="button"
                  onClick={() => setConfirmDiscard(false)}
                  className="rounded-md border border-line-strong bg-bg-3 px-3 py-1.5 text-fg-2 hover:bg-bg-4"
                  style={{ fontSize: "11.5px" }}
                >
                  Keep editing
                </button>
                <button
                  type="button"
                  onClick={onClose}
                  data-testid="paste-skill-discard"
                  className="rounded-md bg-st-failed px-3 py-1.5 font-medium text-white hover:opacity-90"
                  style={{ fontSize: "11.5px" }}
                >
                  Discard
                </button>
              </>
            ) : (
              <>
                <button
                  type="button"
                  onClick={requestClose}
                  className="rounded-md border border-line-strong bg-bg-3 px-3 py-1.5 text-fg-2 hover:bg-bg-4"
                  style={{ fontSize: "11.5px" }}
                >
                  Cancel
                </button>
                <button
                  type="button"
                  onClick={() => void submit()}
                  disabled={!canCreate}
                  data-testid="paste-skill-create"
                  className="rounded-md bg-acc px-3 py-1.5 font-medium text-bg-1 transition-opacity hover:opacity-90 disabled:opacity-40"
                  style={{ fontSize: "11.5px" }}
                >
                  {submitting
                    ? "Creating…"
                    : staged.length > 0
                      ? `Create skill + ${staged.length} file${staged.length === 1 ? "" : "s"}`
                      : "Create skill"}
                </button>
              </>
            )}
          </div>
        </div>
      </div>

      {explorerOpen && (
        <FsExplorerModal
          mode="file"
          multiple
          showHidden
          title="Add files"
          zIndexClass="z-[70]"
          testIdPrefix="skill-file-browse"
          onPick={(path) => acceptHostPicks([path])}
          onPickMany={acceptHostPicks}
          onClose={() => setExplorerOpen(false)}
        />
      )}
    </div>
  );
}

function StagedRow({
  file,
  editing,
  onRemove,
  onRetry,
  onSkip,
}: {
  file: StagedFile;
  editing: boolean;
  onRemove: () => void;
  onRetry: () => void;
  onSkip: () => void;
}) {
  const status = file.status;
  const failed = status.state === "failed";
  const size = status.state === "uploaded" ? status.size : file.size;
  return (
    <li
      className={`flex items-center gap-2 rounded-md border px-3 py-1.5 ${
        failed ? "border-st-failed/60 bg-st-failed-bg" : status.state === "uploading" ? "border-acc/50 bg-bg-3" : "border-line bg-bg-3"
      }`}
      style={{ fontSize: "11px" }}
      data-testid="paste-skill-file"
      data-path={file.path}
      data-state={status.state}
    >
      <FileText size={11} className={`shrink-0 ${failed ? "text-st-failed" : "text-fg-3"}`} />
      <span className="truncate font-mono text-fg">{file.path}</span>
      <StatusBadge status={status} replaces={file.replaces} />
      {failed && <span className="truncate text-fg-2">{status.message}</span>}
      {status.state === "uploading" && (
        <span className="h-1 w-20 overflow-hidden rounded bg-bg-5" aria-hidden>
          <span className="block h-full w-1/2 animate-pulse bg-acc" />
        </span>
      )}
      <span className="flex-1" />
      {failed ? (
        <>
          <button
            type="button"
            onClick={onRetry}
            data-testid="paste-skill-file-retry"
            className="rounded-md border border-line-strong bg-bg-4 px-2 py-0.5 text-fg-2 hover:border-acc"
            style={{ fontSize: "10.5px" }}
          >
            Retry
          </button>
          <button
            type="button"
            onClick={onSkip}
            data-testid="paste-skill-file-skip"
            className="rounded-md border border-line-strong bg-bg-4 px-2 py-0.5 text-fg-2 hover:border-acc"
            style={{ fontSize: "10.5px" }}
          >
            Skip
          </button>
        </>
      ) : (
        size !== null && (
          <span className="font-mono text-fg-4" style={{ fontSize: "10.5px" }}>
            {formatBytes(size)}
          </span>
        )
      )}
      {editing && (
        <button
          type="button"
          aria-label={`Remove ${file.path}`}
          onClick={onRemove}
          className="grid h-4 w-4 place-items-center rounded text-fg-4 hover:text-fg"
        >
          <X size={10} />
        </button>
      )}
    </li>
  );
}

function StatusBadge({ status, replaces }: { status: StagedFile["status"]; replaces?: boolean }) {
  const base = "rounded border px-1.5";
  const style = { fontSize: "9.5px" };
  switch (status.state) {
    case "staged":
      return (
        <span className={`${base} border-acc/50 text-acc`} style={style}>
          {replaces ? "replaces" : "to upload"}
        </span>
      );
    case "uploading":
      return (
        <span className={`${base} border-acc/50 text-acc`} style={style}>
          uploading
        </span>
      );
    case "uploaded":
      return (
        <span className={`${base} border-line text-fg-4`} style={style}>
          uploaded
        </span>
      );
    case "failed":
      return (
        <span className={`${base} border-st-failed/60 text-st-failed`} style={style}>
          failed
        </span>
      );
    case "skipped":
      return (
        <span className={`${base} border-line text-fg-4`} style={style}>
          skipped
        </span>
      );
  }
}

function failingOther(checks: { id: CheckId; state: CheckState }[]): boolean {
  return checks.some((check) => check.id !== "unique" && check.state === "fail");
}

function CheckDot({ state }: { state: CheckState }) {
  if (state === "pass") {
    return (
      <span className="grid h-3.5 w-3.5 place-items-center rounded-full bg-acc/20 text-acc">
        <Check size={9} strokeWidth={3} />
      </span>
    );
  }
  if (state === "fail") {
    return (
      <span className="grid h-3.5 w-3.5 place-items-center rounded-full bg-st-failed/20 text-st-failed">
        <X size={9} strokeWidth={3} />
      </span>
    );
  }
  if (state === "warn") {
    return <span className="grid h-3.5 w-3.5 place-items-center rounded-full bg-st-await/20 text-st-await font-bold" style={{ fontSize: 9 }}>!</span>;
  }
  return <span className="h-3.5 w-3.5 rounded-full border border-line-strong" />;
}
