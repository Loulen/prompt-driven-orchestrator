import { useEffect, useMemo } from "react";
import { Copy, FolderInput, Lock, Pencil, Puzzle, Trash2 } from "lucide-react";
import type { Skill } from "../../shared/generated/Skill";
import type { SkillDetail } from "../model/skill";
import type { SkillFolder } from "../../shared/generated/SkillFolder";
import { timeAgo } from "../model/skillMd";
import { displaySourceUrl, shortCommit } from "../model/skillSource";
import { shortId } from "../model/skillTree";
import { useSkillFiles } from "../store/useSkillFiles";
import { useFileDropTarget } from "../store/useFileDropTarget";
import { isLocked } from "../store/useSkillBankPanel";
import { tildify } from "../../lib/homePath";
import SkillFilesTab from "./SkillFilesTab";
import { DropOverlay } from "./SkillFileDropZone";
import { ActionButton, FolderPicker, TabButton } from "./BankControls";
import SkillMdTab from "./SkillMdTab";
import ExtensionDetailTab from "../../shared/extensions/ExtensionDetailTab";
import { useDetailTabs } from "../../shared/extensions/catalogue";

/** A skill of the bank, read-only: its header, its actions, its SKILL.md and Files tabs (#668, #671). */
export default function SkillDetailView({
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
  sourceFolder,
  onOpenSourceFolder,
  home,
}: {
  skill: Skill;
  detail: SkillDetail | null;
  folders: SkillFolder[];
  /** `skill`, `files`, or the key of a `skill/detail` tab of an extension. */
  tab: string;
  onTab: (tab: string) => void;
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
  /** The Source folder this skill was imported into, if it still exists. */
  sourceFolder: SkillFolder | null;
  onOpenSourceFolder: (folderId: string) => void;
  /** The daemon's home: a local source reads `~/…`. */
  home: string | null;
}) {
  const current = detail && detail.id === skill.id ? detail : null;
  // ADR-0081 (#31): the `skill/detail` tabs extensions add, after Files.
  const entity = useMemo(
    () => ({ kind: "skill", id: skill.id, name: skill.name, attributes: {} }),
    [skill.id, skill.name],
  );
  const { tabs: extensionTabs, context: extensionContext } = useDetailTabs(entity, "skill/detail");
  const extensionTab = extensionTabs.find((t) => t.key === tab) ?? null;
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
  // the pane with the drop overlay (#671 design 02/05) — never on a locked
  // skill, whose files refuse every write.
  const locked = isLocked(skill);
  // #33: a skill an extension provides, seeded with the extension's version.
  const provider = skill.extension ?? null;
  const { dragging, handlers: dropHandlers } = useFileDropTarget(
    locked
      ? () => {}
      : (dataTransfer) => void files.acceptDrop(dataTransfer),
    locked
      ? undefined
      : () => {
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
      {dragging !== null && !locked && (
        <DropOverlay count={dragging} hint="A SKILL.md replaces the skill text · folders are refused" />
      )}
      <div className="flex items-baseline gap-2">
        <h3 className="truncate font-semibold text-fg" style={{ fontSize: "17px" }} data-testid="skill-detail-name">
          {skill.name}
        </h3>
        {locked && provider && (
          <span
            className="flex shrink-0 items-center gap-1 rounded border border-line-strong bg-bg-3 px-1.5 py-0.5 text-fg-3"
            style={{ fontSize: "9.5px" }}
            data-testid="skill-detail-extension"
            title={`Provided by the extension “${provider.id}”: locked against editing and deletion`}
          >
            <Puzzle size={10} />
            Extension {provider.id}
          </span>
        )}
        {locked && provider && (
          <span
            className="shrink-0 rounded border border-line bg-bg-3 px-1.5 py-0.5 font-mono text-fg-4"
            style={{ fontSize: "9.5px" }}
            data-testid="skill-detail-version"
            title="Version of the extension: PDO rewrites the skill at startup when it changes"
          >
            v{provider.version}
          </span>
        )}
        {locked && !provider && (
          <span
            className="flex shrink-0 items-center gap-1 rounded border border-line-strong bg-bg-3 px-1.5 py-0.5 text-fg-3"
            style={{ fontSize: "9.5px" }}
            data-testid="skill-detail-locked"
            title="Seeded by PDO at startup: locked against editing and deletion"
          >
            <Lock size={10} />
            Managed by PDO
          </span>
        )}
        {locked && !provider && frontmatter?.skill_version != null && (
          <span
            className="shrink-0 rounded border border-line bg-bg-3 px-1.5 py-0.5 font-mono text-fg-4"
            style={{ fontSize: "9.5px" }}
            data-testid="skill-detail-version"
            title="Version of the built-in copy: PDO rewrites the skill at startup when it changes"
          >
            v{String(frontmatter.skill_version)}
          </span>
        )}
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
        {skill.source ? (
          <span className="flex items-center gap-1" data-testid="skill-detail-provenance">
            imported from{" "}
            {sourceFolder ? (
              <button
                type="button"
                onClick={() => onOpenSourceFolder(sourceFolder.id)}
                className="font-mono text-fg-3 hover:text-acc hover:underline"
                title={`Open the Source folder “${sourceFolder.name}”`}
              >
                {tildify(displaySourceUrl(skill.source.url), home)}
                {skill.source.commit ? `@${shortCommit(skill.source.commit)}` : ""} · {skill.source.path || "."}
              </button>
            ) : (
              <span className="font-mono text-fg-3" title={skill.source.url}>
                {tildify(displaySourceUrl(skill.source.url), home)}
                {skill.source.commit ? `@${shortCommit(skill.source.commit)}` : ""} · {skill.source.path || "."}
              </span>
            )}
          </span>
        ) : provider ? (
          <span data-testid="skill-detail-provided-by">provided by the extension {provider.id}</span>
        ) : (
          <span>created by paste</span>
        )}
        <span>referenced by 0 tiers</span>
      </div>

      <div className="mt-3 flex items-center gap-2">
        {locked ? (
          <span className="text-fg-4" style={{ fontSize: "10.5px" }} data-testid="skill-detail-locked-note">
            {provider
              ? `Provided by the extension ${provider.id} — it updates with the extension; editing and deletion are refused.`
              : "Seeded by PDO at startup — it updates itself; editing and deletion are refused."}
          </span>
        ) : (
          <>
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
          </>
        )}
        <span className="flex-1" />
        {!locked && (
          <ActionButton icon={<Trash2 size={11} />} label="Delete…" onClick={onDelete} danger testid="skill-detail-delete" />
        )}
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
        {extensionTabs.map((t) => (
          <TabButton key={t.key} active={tab === t.key} onClick={() => onTab(t.key)} label={t.contribution.label} />
        ))}
      </div>

      {extensionTab ? (
        <div className="min-h-0 flex-1 overflow-y-auto">
          <ExtensionDetailTab key={`${extensionTab.key}:${skill.id}`} tab={extensionTab} context={extensionContext} />
        </div>
      ) : tab === "skill" || tab !== "files" ? (
        <SkillMdTab current={current} frontmatter={frontmatter} frontmatterKeys={frontmatterKeys} />
      ) : (
        <SkillFilesTab files={files} pathLabel={pathLabel} leaveRequest={leaveRequest} onLeaveSettled={onLeaveSettled} locked={locked} />
      )}
    </div>
  );
}
