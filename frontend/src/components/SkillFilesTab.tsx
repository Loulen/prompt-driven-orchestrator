import { useState } from "react";
import { FileText, Pencil, Trash2 } from "lucide-react";
import { SKILL_MD, formatBytes } from "../lib/skillFiles";
import type { SkillFiles } from "../hooks/useSkillFiles";
import FsExplorerModal from "./FsExplorerModal";
import SkillFileDropZone from "./SkillFileDropZone";

interface Props {
  files: SkillFiles;
  /** The disk path reminder of the footer (already relativised). */
  pathLabel: string | null;
  /**
   * Set when the user tried to leave (another file, skill, or tab) with an
   * unsaved draft: the editor header asks Save / Discard / Stay.
   */
  leaveRequest: (() => void) | null;
  onLeaveSettled: () => void;
}

/**
 * The Files tab of the skill detail (#671 design 05/07): the rows (SKILL.md
 * first, then the reference files, sub-folders kept in the name), an inline
 * Keep / Delete on the trash, the drop bar with Browse…, and — when a row is
 * clicked — the plain-text editor beside the list.
 */
export default function SkillFilesTab({ files, pathLabel, leaveRequest, onLeaveSettled }: Props) {
  const { detail, editor } = files;
  const skillMdSize = detail?.content ? new TextEncoder().encode(detail.content).length : null;
  const editorOpen = editor !== null;

  const rows: { path: string; size: number | null; isSkillMd: boolean }[] = [
    { path: SKILL_MD, size: skillMdSize, isSkillMd: true },
    ...(detail?.files ?? []).map((file) => ({ path: file.path, size: file.size, isSkillMd: false })),
  ];

  const [pendingOpen, setPendingOpen] = useState<string | null>(null);
  const requestOpen = (path: string) => {
    if (files.dirty && editor && editor.path !== path) {
      // Ask first; the editor header shows Save / Discard / Stay.
      setPendingOpen(path);
      return;
    }
    files.openFile(path);
  };
  const leave: (() => void) | null = leaveRequest ?? (pendingOpen ? () => files.openFile(pendingOpen) : null);
  const settleLeave = () => {
    setPendingOpen(null);
    onLeaveSettled();
  };

  return (
    <div className={`mt-3 flex min-h-0 flex-1 gap-3 ${editorOpen ? "" : "flex-col"}`} data-testid="skill-files">
      <div className={`flex flex-col gap-1 ${editorOpen ? "w-[300px] shrink-0" : ""}`}>
        {rows.map((row) => (
          <FileRow
            key={row.path}
            path={row.path}
            size={row.size}
            active={editor?.path === row.path}
            badge={
              files.replacedByDrop === row.path
                ? { text: "replaced by drop", tone: "acc", detail: "5 / 5 checks pass · saved" }
                : files.justAdded.has(row.path)
                  ? { text: "just added", tone: "acc" }
                  : null
            }
            compact={editorOpen}
            deletable={!row.isSkillMd}
            confirming={files.confirmDeleteFor === row.path}
            onOpen={() => requestOpen(row.path)}
            onAskDelete={() => files.setConfirmDeleteFor(row.path)}
            onKeep={() => files.setConfirmDeleteFor(null)}
            onDelete={() => void files.confirmDelete(row.path)}
          />
        ))}
        {files.refused.map((item) => (
          <div
            key={`refused-${item.name}`}
            className="flex items-center gap-2 rounded-md border border-st-failed/60 bg-st-failed-bg px-3 py-1.5"
            style={{ fontSize: "11px" }}
            data-testid="skill-file-refused"
          >
            <span className="font-mono text-fg">{item.name}</span>
            <span className="rounded border border-st-failed/60 px-1.5 text-st-failed" style={{ fontSize: "9.5px" }}>
              refused
            </span>
            <span className="truncate text-fg-2">{item.reason}</span>
            <span className="flex-1" />
            <button
              type="button"
              aria-label={`Dismiss ${item.name}`}
              onClick={() => files.dismissRefused(item)}
              className="text-fg-4 hover:text-fg"
              style={{ fontSize: "10.5px" }}
            >
              ✕
            </button>
          </div>
        ))}
        <div className="mt-1">
          <SkillFileDropZone
            compact={editorOpen}
            testId="skill-files-drop"
            onBrowse={() => files.setExplorerOpen(true)}
            onPickFiles={files.acceptPickedFiles}
            label={editorOpen ? "Drop or" : "Drop files here to add them (a SKILL.md replaces the skill text), or"}
          />
        </div>
        {!editorOpen && (
          <p className="mt-1 text-fg-4" style={{ fontSize: "10.5px" }} data-testid="skill-files-footer">
            Click a file to edit it as plain text · adding, saving and deleting write to disk immediately
            {pathLabel ? (
              <>
                {" · "}
                <span className="font-mono">{pathLabel}</span>
              </>
            ) : null}
          </p>
        )}
      </div>

      {editor && (
        <FileEditor
          files={files}
          leave={leave}
          onLeaveSettled={settleLeave}
        />
      )}
      {!editorOpen && (
        <p className="sr-only">Switching file or skill with unsaved changes asks first · SKILL.md edits re-run the five checks before saving</p>
      )}

      {files.explorerOpen && (
        <FsExplorerModal
          mode="file"
          multiple
          showHidden
          title="Add files"
          testIdPrefix="skill-file-browse"
          onPick={(path) => void files.acceptHostPicks([path])}
          onPickMany={(paths) => void files.acceptHostPicks(paths)}
          onClose={() => files.setExplorerOpen(false)}
        />
      )}
    </div>
  );
}

function FileRow({
  path,
  size,
  active,
  badge,
  compact,
  deletable,
  confirming,
  onOpen,
  onAskDelete,
  onKeep,
  onDelete,
}: {
  path: string;
  size: number | null;
  active: boolean;
  badge: { text: string; tone: "acc"; detail?: string } | null;
  compact: boolean;
  deletable: boolean;
  confirming: boolean;
  onOpen: () => void;
  onAskDelete: () => void;
  onKeep: () => void;
  onDelete: () => void;
}) {
  return (
    <div
      className={`flex items-center gap-2 rounded-md border px-3 py-1.5 ${
        confirming ? "border-st-failed/60 bg-st-failed-bg" : active ? "border-acc/60 bg-bg-3" : badge ? "border-acc/40 bg-bg-3" : "border-line bg-bg-3"
      }`}
      style={{ fontSize: "11.5px" }}
      data-testid="skill-file-row"
      data-path={path}
    >
      <button
        type="button"
        onClick={onOpen}
        className="flex min-w-0 flex-1 items-center gap-2 text-left"
        aria-label={`Edit ${path}`}
        data-testid="skill-file-open"
      >
        <FileText size={12} className="shrink-0 text-fg-3" />
        <span className="truncate font-mono text-fg">{path}</span>
        {badge && (
          <>
            <span className="shrink-0 rounded border border-acc/50 px-1.5 text-acc" style={{ fontSize: "9.5px" }}>
              {badge.text}
            </span>
            {badge.detail && !compact && (
              <span className="truncate text-fg-4" style={{ fontSize: "10.5px" }}>
                {badge.detail}
              </span>
            )}
          </>
        )}
      </button>
      {confirming ? (
        <span className="flex items-center gap-1.5" data-testid="skill-file-delete-confirm">
          <span className="text-fg-2" style={{ fontSize: "11px" }}>
            Delete this file?
          </span>
          <button
            type="button"
            onClick={onKeep}
            className="rounded-md border border-line-strong bg-bg-4 px-2 py-0.5 text-fg-2 hover:border-acc"
            style={{ fontSize: "10.5px" }}
          >
            Keep
          </button>
          <button
            type="button"
            onClick={onDelete}
            data-testid="skill-file-delete-yes"
            className="rounded-md bg-st-failed px-2 py-0.5 font-medium text-white hover:opacity-90"
            style={{ fontSize: "10.5px" }}
          >
            Delete
          </button>
        </span>
      ) : (
        <>
          {size !== null && (
            <span className="shrink-0 font-mono text-fg-4" style={{ fontSize: "10.5px" }}>
              {formatBytes(size)}
            </span>
          )}
          <button
            type="button"
            onClick={onOpen}
            aria-label={`Edit ${path} as plain text`}
            className="grid h-5 w-5 shrink-0 place-items-center rounded text-fg-4 hover:bg-bg-4 hover:text-fg"
          >
            <Pencil size={11} />
          </button>
          {deletable && (
            <button
              type="button"
              onClick={onAskDelete}
              aria-label={`Delete ${path}`}
              data-testid="skill-file-delete"
              className="grid h-5 w-5 shrink-0 place-items-center rounded text-fg-4 hover:bg-bg-4 hover:text-st-failed"
            >
              <Trash2 size={11} />
            </button>
          )}
        </>
      )}
    </div>
  );
}

function FileEditor({
  files,
  leave,
  onLeaveSettled,
}: {
  files: SkillFiles;
  leave: (() => void) | null;
  onLeaveSettled: () => void;
}) {
  const editor = files.editor!;
  const binary = editor.content?.binary ?? false;
  const dirty = files.dirty;
  const isSkillMd = editor.path === SKILL_MD;

  const onKeyDown = (event: React.KeyboardEvent) => {
    if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "s") {
      event.preventDefault();
      void files.save();
    }
  };

  return (
    <div
      className="flex min-h-[320px] min-w-0 flex-1 flex-col overflow-hidden rounded-md border border-line bg-bg-3"
      data-testid="skill-file-editor"
      data-path={editor.path}
      data-dirty={dirty || undefined}
      onKeyDown={onKeyDown}
    >
      <div className="flex items-center gap-2 border-b border-line px-3 py-2" style={{ fontSize: "11px" }}>
        <span
          className={`h-2 w-2 shrink-0 rounded-full ${dirty ? "bg-st-await" : "bg-acc/60"}`}
          aria-label={dirty ? "unsaved" : "saved"}
          data-testid="skill-file-editor-dot"
        />
        <span className="truncate font-mono text-fg">{editor.path}</span>
        {dirty && <span className="text-fg-4">· unsaved</span>}
        <span className="flex-1" />
        {leave ? (
          <span className="flex items-center gap-1.5" data-testid="skill-file-leave-prompt">
            <span className="text-fg-2">Unsaved changes.</span>
            <button
              type="button"
              onClick={() => {
                void files.save().then((ok) => {
                  if (ok) {
                    leave();
                    onLeaveSettled();
                  }
                });
              }}
              className="rounded-md bg-acc px-2 py-0.5 font-medium text-bg-1 hover:opacity-90"
              style={{ fontSize: "10.5px" }}
              data-testid="skill-file-leave-save"
            >
              Save
            </button>
            <button
              type="button"
              onClick={() => {
                files.revert();
                leave();
                onLeaveSettled();
              }}
              className="rounded-md border border-line-strong bg-bg-4 px-2 py-0.5 text-fg-2 hover:border-st-failed"
              style={{ fontSize: "10.5px" }}
              data-testid="skill-file-leave-discard"
            >
              Discard
            </button>
            <button
              type="button"
              onClick={onLeaveSettled}
              className="rounded-md border border-line-strong bg-bg-4 px-2 py-0.5 text-fg-2 hover:border-acc"
              style={{ fontSize: "10.5px" }}
              data-testid="skill-file-leave-stay"
            >
              Stay
            </button>
          </span>
        ) : (
          <span className="text-fg-4" style={{ fontSize: "10.5px" }}>
            plain text · ⌘S saves
          </span>
        )}
        <button
          type="button"
          onClick={() => {
            if (dirty) return;
            files.closeEditor();
          }}
          disabled={dirty}
          aria-label="Close editor"
          title={dirty ? "Save or revert first" : "Close"}
          className="grid h-5 w-5 place-items-center rounded text-fg-4 hover:bg-bg-4 hover:text-fg disabled:opacity-30"
        >
          ✕
        </button>
      </div>

      {editor.loading ? (
        <div className="p-3 text-fg-4" style={{ fontSize: "11px" }}>
          Loading…
        </div>
      ) : binary ? (
        <div className="flex flex-1 items-center justify-center p-6 text-fg-4" style={{ fontSize: "11.5px" }} data-testid="skill-file-binary">
          binary file · {formatBytes(editor.content?.size ?? 0)}
        </div>
      ) : (
        <textarea
          value={editor.draft}
          onChange={(event) => files.setDraft(event.target.value)}
          spellCheck={false}
          aria-label={`${editor.path} text`}
          aria-invalid={editor.error ? true : undefined}
          data-testid="skill-file-text"
          className="min-h-[220px] flex-1 resize-none bg-bg-1 p-3 font-mono text-fg outline-none"
          style={{ fontSize: "11.5px", lineHeight: 1.55 }}
        />
      )}

      {editor.error && (
        <div
          role="alert"
          className="border-t border-st-failed/50 bg-st-failed-bg px-3 py-2 text-fg-2"
          style={{ fontSize: "11px" }}
          data-testid="skill-file-error"
        >
          <strong className="text-fg">{isSkillMd ? "Cannot save SKILL.md." : "Cannot save."}</strong> {editor.error}
        </div>
      )}

      {!binary && !editor.loading && (
        <div className="flex items-center gap-2 border-t border-line px-3 py-2" style={{ fontSize: "10.5px" }}>
          <span className="text-fg-4">
            {editor.savedAt ? "Saved just now" : "On disk"} · {formatBytes(new TextEncoder().encode(editor.saved).length)}
            {isSkillMd ? " · five checks re-run before saving" : ""}
          </span>
          <span className="flex-1" />
          <button
            type="button"
            onClick={files.revert}
            disabled={!dirty}
            data-testid="skill-file-revert"
            className="rounded-md border border-line-strong bg-bg-4 px-2.5 py-1 text-fg-2 hover:border-acc disabled:opacity-40"
          >
            Revert
          </button>
          <button
            type="button"
            onClick={() => void files.save()}
            disabled={!dirty || files.saving}
            data-testid="skill-file-save"
            className="rounded-md bg-acc px-2.5 py-1 font-medium text-bg-1 hover:opacity-90 disabled:opacity-40"
          >
            {files.saving ? "Saving…" : "Save"}
          </button>
        </div>
      )}
    </div>
  );
}
