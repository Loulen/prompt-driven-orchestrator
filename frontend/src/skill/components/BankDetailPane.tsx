import type { SkillBank } from "../model/skill";
import { shortId } from "../model/skillTree";
import type { SkillBankPanelState } from "../store/useSkillBankPanel";
import { tildify } from "../../lib/homePath";
import { EmptyBank, RescanInProgress } from "./BankPlaceholders";
import { DeleteFolderConfirm, DeleteSkillConfirm } from "./DeleteConfirms";
import FolderDetailView from "./FolderDetailView";
import SkillDetailView from "./SkillDetailView";
import UpdateFromSourceConfirm from "./UpdateFromSourceConfirm";

/**
 * The bank's right pane: the error, then a confirmation in progress, the
 * selected skill or folder, or an empty state — and the undo toast.
 */
export default function BankDetailPane({
  bank,
  loaded,
  home,
  onChanged,
  panel,
}: {
  bank: SkillBank;
  loaded: boolean;
  home: string | null;
  onChanged: () => Promise<void>;
  panel: SkillBankPanelState;
}) {
  const { skills, folders } = bank;
  const { error, pending, selectedSkill, selectedFolder, counts, folderById, toast } = panel;
  const emptyBank = loaded && skills.length === 0 && folders.length === 0;
  return (
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
          path={tildify(`${bank.root_path}/${pending.skill.id}`, home)}
          onCancel={() => panel.setPending(null)}
          onConfirm={() => void panel.confirmDelete()}
        />
      ) : pending?.kind === "delete-folder" ? (
        <DeleteFolderConfirm
          folder={pending.folder}
          count={counts.get(pending.folder.id) ?? 0}
          parentName={pending.folder.parent_id ? folderById.get(pending.folder.parent_id)?.name ?? null : null}
          onCancel={() => panel.setPending(null)}
          onConfirm={() => void panel.confirmDelete()}
        />
      ) : pending?.kind === "rescan-folder" ? (
        <RescanInProgress folder={pending.folder} onCancel={() => panel.setPending(null)} />
      ) : pending?.kind === "update-folder" ? (
        <UpdateFromSourceConfirm
          folder={pending.folder}
          report={pending.report}
          onCancel={() => panel.setPending(null)}
          onConfirm={(items) => void panel.confirmUpdateFromSource(items)}
        />
      ) : selectedSkill ? (
        <SkillDetailView
          skill={selectedSkill}
          detail={panel.detail}
          folders={folders}
          tab={panel.detailTab}
          onTab={(tab) => panel.guarded(() => panel.setDetailTab(tab))}
          existingNames={panel.existingNames}
          refreshDetail={panel.refreshDetail}
          onSkillChanged={onChanged}
          showToast={(message) => panel.showToast({ message })}
          onError={panel.setError}
          pathLabel={bank.root_path ? `${tildify(bank.root_path, home)}/${shortId(selectedSkill.id)}/` : null}
          leaveRequest={panel.leaveRequest}
          onLeaveSettled={() => panel.setLeaveRequest(null)}
          onDirtyChange={panel.setFilesDirty}
          copied={panel.copied}
          onCopyId={() => void panel.copyId(selectedSkill.id)}
          onRename={() => panel.startRename({ kind: "skill", id: selectedSkill.id })}
          onMove={() => panel.setMovePickerFor(selectedSkill.id)}
          onDelete={() => void panel.askDelete({ kind: "skill", id: selectedSkill.id })}
          movePickerOpen={panel.movePickerFor === selectedSkill.id}
          onMoveTo={(folderId) => {
            panel.setMovePickerFor(null);
            void panel.moveSkill(selectedSkill.id, folderId);
          }}
          sourceFolder={folders.find(
            (folder) =>
              folder.source &&
              selectedSkill.source &&
              folder.source.url === selectedSkill.source.url &&
              selectedSkill.source.path.startsWith(folder.source.path),
          ) ?? null}
          onOpenSourceFolder={panel.openSourceFolder}
          home={home}
        />
      ) : selectedFolder ? (
        <FolderDetailView
          folder={selectedFolder}
          folders={folders}
          skills={skills}
          count={counts.get(selectedFolder.id) ?? 0}
          onNewSubfolder={() => void panel.newFolder(selectedFolder.id)}
          onRename={() => panel.startRename({ kind: "folder", id: selectedFolder.id })}
          onDelete={() => void panel.askDelete({ kind: "folder", id: selectedFolder.id })}
          onMoveTo={(parentId) => void panel.moveFolder(selectedFolder.id, parentId)}
          onUpdateFromSource={() => void panel.startUpdateFromSource(selectedFolder)}
          onExport={() => void panel.exportBundle({ skill_ids: [], folder_ids: [selectedFolder.id] })}
          home={home}
        />
      ) : emptyBank ? (
        <EmptyBank onPaste={() => panel.setPasteOpen(true)} onImport={() => panel.setImportOpen(true)} />
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
                panel.setToast(null);
                if (panel.toastTimer.current) window.clearTimeout(panel.toastTimer.current);
                void undo?.().catch((cause) => panel.failWith(cause, "Undo failed"));
              }}
              className="font-medium text-acc hover:underline"
            >
              Undo
            </button>
          )}
        </div>
      )}
    </div>
  );
}
