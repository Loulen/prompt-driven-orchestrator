import { ChevronDown, ClipboardPaste, Download, Folder, Plus, Search } from "lucide-react";
import type { SkillBankPanelState } from "../store/useSkillBankPanel";
import { MenuItem } from "./BankControls";

/** Above the tree: the filter, the new-folder button and the Add menu (paste or import). */
export default function BankToolbar({ panel }: { panel: SkillBankPanelState }) {
  const { selectedFolder, selectedSkill, addMenuOpen } = panel;
  return (
    <div className="flex items-center gap-2 px-3 py-2.5">
      <label className="flex min-w-0 flex-1 items-center gap-1.5 rounded-md border border-line-strong bg-bg-3 px-2 py-1.5">
        <Search size={12} className="shrink-0 text-fg-4" />
        <input
          value={panel.filter}
          onChange={(event) => panel.setFilter(event.target.value)}
          placeholder="Filter skills…"
          aria-label="Filter skills"
          data-testid="skill-filter"
          className="min-w-0 flex-1 bg-transparent text-fg outline-none placeholder:text-fg-4"
          style={{ fontSize: "11.5px" }}
        />
      </label>
      <button
        type="button"
        onClick={() => void panel.newFolder(selectedFolder?.id ?? (selectedSkill?.folder_id ?? null))}
        aria-label="New folder"
        title="New folder"
        data-testid="skill-new-folder"
        className="flex shrink-0 items-center gap-1 rounded-md border border-line-strong bg-bg-3 px-2 py-1.5 text-fg-2 hover:border-acc"
      >
        <Folder size={12} />
        <span style={{ fontSize: "12px", lineHeight: 1 }}>+</span>
      </button>
      <span className="relative">
        <button
          type="button"
          onClick={(event) => {
            event.stopPropagation();
            panel.setMenuFor(null);
            panel.setAddMenuOpen((open) => !open);
          }}
          aria-haspopup="menu"
          aria-expanded={addMenuOpen}
          data-testid="skill-add"
          className="flex shrink-0 items-center gap-1 rounded-md bg-acc px-2.5 py-1.5 font-medium text-bg-1 hover:opacity-90"
          style={{ fontSize: "11.5px" }}
        >
          <Plus size={12} />
          Add
          <ChevronDown size={11} />
        </button>
        {addMenuOpen && (
          <div
            role="menu"
            data-testid="skill-add-menu"
            className="absolute right-0 top-8 z-10 w-64 rounded-md border border-line bg-bg-4 p-1 shadow-xl"
            onClick={(event) => event.stopPropagation()}
          >
            <MenuItem
              icon={<ClipboardPaste size={12} />}
              label="Paste SKILL.md…"
              testid="skill-paste"
              onClick={() => {
                panel.setAddMenuOpen(false);
                panel.setPasteOpen(true);
              }}
            />
            <MenuItem
              icon={<Download size={12} />}
              label="Import from a source…"
              hint="repo · folder"
              testid="skill-import"
              onClick={() => {
                panel.setAddMenuOpen(false);
                panel.setImportOpen(true);
              }}
            />
          </div>
        )}
      </span>
    </div>
  );
}
