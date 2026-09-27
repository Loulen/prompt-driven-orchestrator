import { Folder, ClipboardPaste, X } from "lucide-react";
import type { SkillFolder } from "../../shared/generated/SkillFolder";
import type { Skill } from "../../shared/generated/Skill";
import { formatBytes, totalStagedBytes } from "../model/skillFiles";
import { usePasteSkill } from "../store/usePasteSkill";
import FsExplorerModal from "../../workspace/components/FsExplorerModal";
import SkillFileDropZone, { DropOverlay } from "./SkillFileDropZone";
import PasteSkillSidebar from "./PasteSkillSidebar";
import StagedRow from "./StagedFileRow";

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
  const paste = usePasteSkill({ existingNames, initialFolderId, onClose, onCreated });
  const {
    text,
    setText,
    confirmDiscard,
    setConfirmDiscard,
    textareaRef,
    staged,
    refused,
    setRefused,
    replacedFrom,
    setReplacedFrom,
    explorerOpen,
    setExplorerOpen,
    phase,
    setServerError,
    validation,
    failing,
    canCreate,
    submitting,
    editing,
    busy,
    finish,
    requestClose,
    acceptFiles,
    acceptHostPicks,
    removeStaged,
    dragging,
    dropHandlers,
    submit,
    retry,
    skip,
  } = paste;

  // ---- rendering ----------------------------------------------------------

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
          <PasteSkillSidebar paste={paste} folders={folders} />
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
