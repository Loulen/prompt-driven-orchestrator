import type { SkillBank } from "../model/skill";
import type { TreeRow } from "../model/skillTree";
import { checkState, skillIdsOf } from "../model/skillExport";
import { useSkillBankPanel } from "../store/useSkillBankPanel";
import { tildify } from "../../lib/homePath";
import SkillTree from "./SkillTree";
import PasteSkillModal from "./PasteSkillModal";
import ImportSkillsModal from "./ImportSkillsModal";
import BankToolbar from "./BankToolbar";
import BankDetailPane from "./BankDetailPane";
import TreeRowActions from "./TreeRowActions";
import TreeRowCheck from "./TreeRowCheck";
import ExportSelectionBar from "./ExportSelectionBar";

interface Props {
  bank: SkillBank;
  loaded: boolean;
  /** Host `$HOME`, to shorten the disk path in the footer. `null` shows it as is. */
  home: string | null;
  /** Called after every successful write; the parent refetches and fires the bus. */
  onChanged: () => Promise<void>;
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
  const panel = useSkillBankPanel(bank, home, onChanged);
  const { selectedFolder, selectedSkill, filter } = panel;

  const rootLabel = bank.root_path ? `${tildify(bank.root_path, home)}/<id>/` : "";
  const renderActions = (row: TreeRow) => <TreeRowActions row={row} panel={panel} />;

  return (
    <div className="flex min-h-0 flex-1" data-testid="skill-bank-panel">
      {/* Left: tree */}
      <div className="flex w-[300px] shrink-0 flex-col border-r border-line">
        <BankToolbar panel={panel} />

        <div className="flex min-h-0 flex-1 flex-col overflow-y-auto">
          <SkillTree
            folders={folders}
            skills={skills}
            selected={panel.selectedRef}
            onSelect={(ref) =>
              panel.guarded(() => {
                panel.setSelected(ref);
                panel.setPending(null);
                panel.setMenuFor(null);
              })
            }
            expanded={panel.expanded}
            onToggle={panel.toggleExpanded}
            filter={filter}
            renaming={panel.renaming}
            renameError={panel.renameError}
            onRenameCommit={panel.commitRename}
            onRenameCancel={() => {
              panel.setRenaming(null);
              panel.setRenameError(null);
            }}
            onDropSkill={(skillId, folderId) => void panel.moveSkill(skillId, folderId)}
            renderActions={renderActions}
            renderLeading={(row) => <TreeRowCheck row={row} bank={bank} panel={panel} />}
            isMarked={(row) => checkState(panel.checked, skillIdsOf(row.ref, bank)) === "all"}
            onRequestRename={panel.startRename}
            onRequestDelete={(ref) => void panel.askDelete(ref)}
            emptyState={
              !loaded ? (
                <div className="px-4 py-6 text-text-3" style={{ fontSize: "11px" }}>
                  Loading…
                </div>
              ) : filter.trim() ? (
                <div className="px-4 py-6 text-text-3" style={{ fontSize: "11px" }}>
                  No skill matches “{filter.trim()}”.
                </div>
              ) : null
            }
          />
        </div>

        {panel.checkedCount > 0 ? (
          <ExportSelectionBar bank={bank} panel={panel} />
        ) : (
        <div
          className="flex items-center justify-between gap-2 border-t border-line px-3 py-2 text-text-3"
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
        )}
      </div>

      {/* Right: detail / confirmation / empty state */}
      <BankDetailPane bank={bank} loaded={loaded} home={home} onChanged={onChanged} panel={panel} />

      {panel.importOpen && (
        <ImportSkillsModal
          folders={folders}
          existingNames={panel.existingNames}
          initialFolderId={selectedFolder?.id ?? selectedSkill?.folder_id ?? null}
          home={home}
          onClose={() => panel.setImportOpen(false)}
          onImported={async (report, complete) => {
            await onChanged();
            panel.expandTo(report.folder.id);
            panel.setSelected({ kind: "folder", id: report.folder.id });
            panel.setPending(null);
            if (complete) {
              const n = report.imported.length;
              panel.showToast({ message: `Imported ${n} skill${n === 1 ? "" : "s"} into ${report.folder.name}` });
            }
          }}
        />
      )}

      {panel.pasteOpen && (
        <PasteSkillModal
          folders={folders}
          existingNames={panel.existingNames}
          initialFolderId={selectedFolder?.id ?? selectedSkill?.folder_id ?? null}
          onClose={() => panel.setPasteOpen(false)}
          onCreated={async (skill, fileCount) => {
            await onChanged();
            panel.expandTo(skill.folder_id ?? null);
            panel.setSelected({ kind: "skill", id: skill.id });
            // Files attached: land on the Files tab, so FP #671 step 2 reads
            // without a click.
            panel.setDetailTab(fileCount > 0 ? "files" : "skill");
          }}
        />
      )}
    </div>
  );
}
