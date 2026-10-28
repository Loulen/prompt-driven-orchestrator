import { useState } from "react";
import { SKILL_MD } from "../model/skillFiles";
import type { SkillFiles } from "../store/useSkillFiles";
import FsExplorerModal from "../../workspace/components/FsExplorerModal";
import SkillFileDropZone from "./SkillFileDropZone";
import FileRow from "./SkillFileRow";
import FileEditor from "./SkillFileEditor";

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
  /** The seeded skill (#722): files are read-only — no drop, no add, no delete, no save. */
  locked?: boolean;
}

/**
 * The Files tab of the skill detail (#671 design 05/07): the rows (SKILL.md
 * first, then the reference files, sub-folders kept in the name), an inline
 * Keep / Delete on the trash, the drop bar with Browse…, and — when a row is
 * clicked — the plain-text editor beside the list.
 */
export default function SkillFilesTab({ files, pathLabel, leaveRequest, onLeaveSettled, locked = false }: Props) {
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
            deletable={!row.isSkillMd && !locked}
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
            className="flex items-center gap-2 rounded-md border border-st-failed/60 bg-st-failed/14 px-3 py-1.5"
            style={{ fontSize: "11px" }}
            data-testid="skill-file-refused"
          >
            <span className="font-mono text-text">{item.name}</span>
            <span className="rounded border border-st-failed/60 px-1.5 text-st-failed" style={{ fontSize: "9.5px" }}>
              refused
            </span>
            <span className="truncate text-text-2">{item.reason}</span>
            <span className="flex-1" />
            <button
              type="button"
              aria-label={`Dismiss ${item.name}`}
              onClick={() => files.dismissRefused(item)}
              className="text-text-3 hover:text-text"
              style={{ fontSize: "10.5px" }}
            >
              ✕
            </button>
          </div>
        ))}
        <div className="mt-1">
          {!locked && (
            <SkillFileDropZone
              compact={editorOpen}
              testId="skill-files-drop"
              onBrowse={() => files.setExplorerOpen(true)}
              onPickFiles={files.acceptPickedFiles}
              label={editorOpen ? "Drop or" : "Drop files here to add them (a SKILL.md replaces the skill text), or"}
            />
          )}
        </div>
        {!editorOpen && (
          <p className="mt-1 text-text-3" style={{ fontSize: "10.5px" }} data-testid="skill-files-footer">
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
          locked={locked}
        />
      )}
      {!editorOpen && (
        <p className="sr-only">Switching file or skill with unsaved changes asks first · SKILL.md edits re-run the five checks before saving</p>
      )}

      {files.explorerOpen && !locked && (
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
