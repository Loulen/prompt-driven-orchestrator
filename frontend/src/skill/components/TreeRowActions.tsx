import { useMemo, type SyntheticEvent } from "react";
import { Copy, Download, FolderInput, FolderPlus, Lock, MoreVertical, Pencil, Trash2 } from "lucide-react";
import { ExtensionMenuGroups } from "../../shared/extensions/ExtensionActions";
import type { SkillEntity } from "../../shared/extensions/contract";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "../../shared/ui/dropdown-menu";
import { sameRef, type TreeRow } from "../model/skillTree";
import type { SkillBankPanelState } from "../store/useSkillBankPanel";

/** The panel closes its menus on any click that reaches `window`: the menu's own clicks never do. */
const stop = (event: SyntheticEvent) => event.stopPropagation();

/** The extensions' entries of the tree's menu. */
const EXTENSION_ITEM = "cursor-pointer gap-2 px-2 py-1.5 text-sm text-text-2";

/**
 * The hover / focus actions at the right of a tree row: the pencil, and the
 * kebab with the row's verbs (a locked skill only offers its id), on the shared
 * `DropdownMenu` (#30), then the extensions' `skill/menu` groups.
 */
export default function TreeRowActions({ row, panel }: { row: TreeRow; panel: SkillBankPanelState }) {
  const ref = row.ref;
  const isMenuOpen = sameRef(panel.menuFor, ref);
  const lockedSkill = row.skill?.locked ?? false;
  const name = row.folder?.name ?? row.skill?.name ?? "";
  const extra = useMemo(() => {
    const entity: SkillEntity = {
      kind: ref.kind === "folder" ? "skill_folder" : "skill",
      id: ref.id,
      name,
      attributes: {},
    };
    return { entity };
  }, [ref.kind, ref.id, name]);
  const exportItem = (
    <TreeMenuItem
      icon={<Download size={12} />}
      label="Export"
      hint=".zip"
      testid={`tree-menu-export-${ref.kind}-${ref.id}`}
      onClick={() =>
        void panel.exportBundle(
          ref.kind === "folder" ? { skill_ids: [], folder_ids: [ref.id] } : { skill_ids: [ref.id], folder_ids: [] },
        )
      }
    />
  );
  const extensions = <ExtensionMenuGroups location="skill/menu" extra={extra} itemClassName={EXTENSION_ITEM} />;
  return (
    <>
      {!lockedSkill && (
        <button
          type="button"
          aria-label={`Rename ${name}`}
          onClick={() => panel.startRename(ref)}
          className="grid h-5 w-5 place-items-center rounded text-text-3 hover:bg-hover hover:text-text"
        >
          <Pencil size={11} />
        </button>
      )}
      <DropdownMenu
        open={isMenuOpen}
        // One click opens another row's menu, as it always did: no modal layer.
        modal={false}
        onOpenChange={(open) => {
          if (open) panel.setMovePickerFor(null);
          panel.setMenuFor(open ? ref : null);
        }}
      >
        <DropdownMenuTrigger
          aria-label={`More actions for ${name}`}
          aria-expanded={isMenuOpen}
          data-testid={`kebab-${ref.kind}-${ref.id}`}
          onClick={stop}
          className="grid h-5 w-5 place-items-center rounded text-text-3 hover:bg-hover hover:text-text"
        >
          <MoreVertical size={11} />
        </DropdownMenuTrigger>
        <DropdownMenuContent
          align="end"
          data-testid="tree-menu"
          className="w-48"
          onClick={stop}
        >
          {ref.kind === "folder" ? (
            <>
              <TreeMenuItem icon={<FolderPlus size={12} />} label="New subfolder" onClick={() => void panel.newFolder(ref.id)} />
              <TreeMenuItem icon={<Pencil size={12} />} label="Rename" hint="F2" onClick={() => panel.startRename(ref)} />
              {exportItem}
              {extensions}
              <DropdownMenuSeparator />
              <TreeMenuItem icon={<Trash2 size={12} />} label="Delete folder" hint="⌫" danger onClick={() => void panel.askDelete(ref)} />
            </>
          ) : lockedSkill ? (
            <>
              <TreeMenuItem icon={<Copy size={12} />} label="Copy id" onClick={() => void panel.copyId(ref.id)} />
              {exportItem}
              {extensions}
              <DropdownMenuSeparator />
              <div
                className="flex items-center gap-2 px-2 py-1.5 text-text-3 text-xs"
                data-testid="tree-menu-locked"
              >
                <Lock size={12} className="shrink-0" />
                {row.skill?.extension ? `Provided by the extension ${row.skill.extension.id}` : "Built in"}
              </div>
            </>
          ) : (
            <>
              <TreeMenuItem icon={<Pencil size={12} />} label="Rename" hint="F2" onClick={() => panel.startRename(ref)} />
              <TreeMenuItem
                icon={<FolderInput size={12} />}
                label="Move to…"
                onClick={() => {
                  panel.setMenuFor(null);
                  panel.setSelected(ref);
                  panel.setMovePickerFor(ref.id);
                }}
              />
              <TreeMenuItem icon={<Copy size={12} />} label="Copy id" onClick={() => void panel.copyId(ref.id)} />
              {exportItem}
              {extensions}
              <DropdownMenuSeparator />
              <TreeMenuItem icon={<Trash2 size={12} />} label="Delete…" hint="⌫" danger onClick={() => void panel.askDelete(ref)} />
            </>
          )}
        </DropdownMenuContent>
      </DropdownMenu>
    </>
  );
}

/** An entry of the tree's menu: icon, label, and its key hint. */
function TreeMenuItem({
  icon,
  label,
  hint,
  danger,
  testid,
  onClick,
}: {
  icon: React.ReactNode;
  label: string;
  hint?: string;
  danger?: boolean;
  testid?: string;
  onClick: () => void;
}) {
  return (
    <DropdownMenuItem
      variant={danger ? "destructive" : "default"}
      data-testid={testid}
      onClick={onClick}
      className="cursor-pointer gap-2 px-2 py-1.5 text-sm text-text-2"
    >
      <span className="shrink-0">{icon}</span>
      <span className="flex-1 whitespace-nowrap">{label}</span>
      {hint && (
        <span className="font-mono text-text-3 text-2xs">
          {hint}
        </span>
      )}
    </DropdownMenuItem>
  );
}
