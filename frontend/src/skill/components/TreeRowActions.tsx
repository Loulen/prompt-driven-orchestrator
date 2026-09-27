import { Copy, FolderInput, FolderPlus, Lock, MoreVertical, Pencil, Trash2 } from "lucide-react";
import { sameRef, type TreeRow } from "../model/skillTree";
import type { SkillBankPanelState } from "../store/useSkillBankPanel";
import { MenuItem, MenuSeparator } from "./BankControls";

/**
 * The hover / focus actions at the right of a tree row: the pencil, and the
 * kebab with the row's verbs (a locked skill only offers its id).
 */
export default function TreeRowActions({ row, panel }: { row: TreeRow; panel: SkillBankPanelState }) {
  const ref = row.ref;
  const isMenuOpen = sameRef(panel.menuFor, ref);
  const lockedSkill = row.skill?.locked ?? false;
  return (
    <>
      {!lockedSkill && (
        <button
          type="button"
          aria-label={`Rename ${row.folder?.name ?? row.skill?.name ?? ""}`}
          onClick={() => panel.startRename(ref)}
          className="grid h-5 w-5 place-items-center rounded text-fg-4 hover:bg-bg-4 hover:text-fg"
        >
          <Pencil size={11} />
        </button>
      )}
      <span className="relative">
        <button
          type="button"
          aria-label={`More actions for ${row.folder?.name ?? row.skill?.name ?? ""}`}
          aria-expanded={isMenuOpen}
          data-testid={`kebab-${ref.kind}-${ref.id}`}
          onClick={(event) => {
            event.stopPropagation();
            panel.setMovePickerFor(null);
            panel.setMenuFor(isMenuOpen ? null : ref);
          }}
          className="grid h-5 w-5 place-items-center rounded text-fg-4 hover:bg-bg-4 hover:text-fg"
        >
          <MoreVertical size={11} />
        </button>
        {isMenuOpen && (
          <div
            role="menu"
            data-testid="tree-menu"
            className="absolute right-0 top-6 z-10 w-48 rounded-md border border-line bg-bg-4 p-1 shadow-xl"
            onClick={(event) => event.stopPropagation()}
          >
            {ref.kind === "folder" ? (
              <>
                <MenuItem icon={<FolderPlus size={12} />} label="New subfolder" onClick={() => void panel.newFolder(ref.id)} />
                <MenuItem icon={<Pencil size={12} />} label="Rename" hint="F2" onClick={() => panel.startRename(ref)} />
                <MenuSeparator />
                <MenuItem icon={<Trash2 size={12} />} label="Delete folder" hint="⌫" danger onClick={() => void panel.askDelete(ref)} />
              </>
            ) : lockedSkill ? (
              <>
                <MenuItem icon={<Copy size={12} />} label="Copy id" onClick={() => void panel.copyId(ref.id)} />
                <MenuSeparator />
                <div
                  className="flex items-center gap-2 px-2 py-1.5 text-fg-4"
                  style={{ fontSize: "11px" }}
                  data-testid="tree-menu-locked"
                >
                  <Lock size={12} className="shrink-0" />
                  Managed by PDO
                </div>
              </>
            ) : (
              <>
                <MenuItem icon={<Pencil size={12} />} label="Rename" hint="F2" onClick={() => panel.startRename(ref)} />
                <MenuItem
                  icon={<FolderInput size={12} />}
                  label="Move to…"
                  onClick={() => {
                    panel.setMenuFor(null);
                    panel.setSelected(ref);
                    panel.setMovePickerFor(ref.id);
                  }}
                />
                <MenuItem icon={<Copy size={12} />} label="Copy id" onClick={() => void panel.copyId(ref.id)} />
                <MenuSeparator />
                <MenuItem icon={<Trash2 size={12} />} label="Delete…" hint="⌫" danger onClick={() => void panel.askDelete(ref)} />
              </>
            )}
          </div>
        )}
      </span>
    </>
  );
}
