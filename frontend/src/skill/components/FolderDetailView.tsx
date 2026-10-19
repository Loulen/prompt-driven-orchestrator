import { useMemo, useState } from "react";
import { Download, Folder, FolderInput, FolderPlus, Pencil, RefreshCw, Trash2 } from "lucide-react";
import type { Skill } from "../../shared/generated/Skill";
import type { SkillFolder } from "../../shared/generated/SkillFolder";
import { timeAgo } from "../model/skillMd";
import { shortCommit } from "../model/skillSource";
import { descendantFolderIds, folderPathLabel } from "../model/skillTree";
import { tildify } from "../../lib/homePath";
import { ActionButton, FolderPicker } from "./BankControls";

/** A folder of the bank: its actions and, for a Source folder, its provenance (#670). */
export default function FolderDetailView({
  folder,
  folders,
  skills,
  count,
  onNewSubfolder,
  onRename,
  onDelete,
  onMoveTo,
  onUpdateFromSource,
  onExport,
  home,
}: {
  folder: SkillFolder;
  folders: SkillFolder[];
  skills: Skill[];
  count: number;
  onNewSubfolder: () => void;
  onRename: () => void;
  onDelete: () => void;
  onMoveTo: (parentId: string | null) => void;
  onUpdateFromSource: () => void;
  /** Download the folder's skills (sub-folders included) as a zip. */
  onExport: () => void;
  /** The daemon's home: a local source reads `~/…`. */
  home: string | null;
}) {
  const [picker, setPicker] = useState(false);
  const exclude = useMemo(() => descendantFolderIds(folder.id, folders), [folder.id, folders]);
  const source = folder.source ?? null;
  const importedHere = source
    ? skills.filter((skill) => skill.folder_id === folder.id && skill.source?.url === source.url).length
    : 0;
  return (
    <div className="flex min-h-0 flex-1 flex-col p-5" data-testid="folder-detail">
      <div className="flex items-center gap-2">
        <Folder size={16} className="text-st-await" />
        <h3 className="truncate font-semibold text-fg" style={{ fontSize: "17px" }}>
          {folderPathLabel(folder.id, folders)}
        </h3>
        {source && (
          <span
            className="rounded border border-acc/40 bg-acc/10 px-1.5 py-0.5 text-acc"
            style={{ fontSize: "9.5px" }}
            data-testid="folder-source-badge"
          >
            Source
          </span>
        )}
        <span className="flex-1" />
        <button
          type="button"
          onClick={onExport}
          disabled={count === 0}
          title={count === 0 ? "No skill in this folder" : undefined}
          data-testid="folder-detail-export"
          className="flex shrink-0 items-center gap-1.5 whitespace-nowrap rounded-md border border-line-strong bg-bg-3 px-2.5 py-1.5 text-fg-2 hover:border-acc disabled:cursor-not-allowed disabled:opacity-45"
          style={{ fontSize: "11.5px" }}
        >
          <Download size={11} />
          Export folder
        </button>
      </div>
      <p className="mt-1.5 text-fg-3" style={{ fontSize: "12px" }}>
        {count} skill{count === 1 ? "" : "s"} in this folder and below. Checking a folder in a
        selector will check its skills at that instant; nothing references a folder.
      </p>
      <p className="mt-1 text-fg-4" style={{ fontSize: "11px" }} data-testid="folder-export-hint">
        The zip holds one folder per skill (<code>SKILL.md</code> + its reference files), the format
        « + Add › Upload a zip » reads back on another instance.
      </p>
      <div className="mt-3 flex items-center gap-2">
        <ActionButton icon={<FolderPlus size={11} />} label="New subfolder" onClick={onNewSubfolder} testid="folder-detail-new" />
        <ActionButton icon={<Pencil size={11} />} label="Rename" onClick={onRename} testid="folder-detail-rename" />
        <span className="relative">
          <ActionButton icon={<FolderInput size={11} />} label="Move to…" onClick={() => setPicker((p) => !p)} />
          {picker && (
            <FolderPicker
              folders={folders}
              exclude={exclude}
              current={folder.parent_id ?? null}
              onPick={(parentId) => {
                setPicker(false);
                onMoveTo(parentId);
              }}
              onClose={() => setPicker(false)}
            />
          )}
        </span>
        <span className="flex-1" />
        <ActionButton icon={<Trash2 size={11} />} label="Delete folder" onClick={onDelete} danger testid="folder-detail-delete" />
      </div>

      {source && (
        <>
          <div className="mt-4 overflow-hidden rounded-md border border-line" data-testid="folder-provenance">
            <div className="flex items-center justify-between bg-bg-3 px-3 py-1.5">
              <span className="text-fg-4 uppercase tracking-wide" style={{ fontSize: "9.5px" }}>
                Provenance
              </span>
              <button
                type="button"
                onClick={onUpdateFromSource}
                data-testid="folder-update-from-source"
                className="flex items-center gap-1 text-acc hover:underline"
                style={{ fontSize: "11px" }}
              >
                <RefreshCw size={10} />
                Update from source…
              </button>
            </div>
            <table className="w-full" style={{ fontSize: "11.5px" }}>
              <tbody>
                <ProvenanceRow label="source">
                  <span className="font-mono text-fg" title={source.url}>
                    {tildify(source.url, home)}
                  </span>
                </ProvenanceRow>
                <ProvenanceRow label="ref">
                  <span className="font-mono text-fg">{source.ref ?? "default branch"}</span>
                </ProvenanceRow>
                <ProvenanceRow label="commit">
                  <span className="font-mono text-fg">{source.commit ? shortCommit(source.commit) : "—"}</span>
                  {source.imported_at && (
                    <span className="ml-2 font-mono text-fg-4" style={{ fontSize: "10.5px" }}>
                      · imported {timeAgo(source.imported_at)}
                    </span>
                  )}
                </ProvenanceRow>
                <ProvenanceRow label="path">
                  <span className="font-mono text-fg">{source.path ? `${source.path}/` : "."}</span>
                </ProvenanceRow>
                <ProvenanceRow label="imported">
                  <span className="text-fg">
                    {importedHere} of {source.found} skill{source.found === 1 ? "" : "s"} found at the source
                  </span>
                  {source.invalid > 0 && (
                    <span className="ml-2 text-fg-4" style={{ fontSize: "10.5px" }}>
                      · {source.invalid} invalid
                    </span>
                  )}
                </ProvenanceRow>
              </tbody>
            </table>
          </div>
          <p className="mt-3 rounded-md border border-dashed border-line px-3 py-2 text-fg-4" style={{ fontSize: "10.5px" }}>
            Renaming or moving this folder keeps its provenance. Deleting it moves its skills to the parent and drops the
            link to the source; the skills keep their own provenance.
          </p>
        </>
      )}
    </div>
  );
}

function ProvenanceRow({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <tr className="border-t border-line">
      <td className="w-[110px] px-3 py-1.5 align-top text-fg-4">{label}</td>
      <td className="px-3 py-1.5">{children}</td>
    </tr>
  );
}
