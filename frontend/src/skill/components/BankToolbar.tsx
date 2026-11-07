import { useRef } from "react";
import { ChevronDown, ClipboardPaste, Download, FileArchive, Folder, Plus, Search } from "lucide-react";
import type { SkillBankPanelState } from "../store/useSkillBankPanel";
import { MenuReasons } from "../../shared/ui/kit/menu";
import { reasonsOf } from "../../shared/ui/menuReasons";
import { GREYED, MenuItem, RefusalTip } from "./BankControls";

/** Above the tree: the filter, the new-folder button and the Add menu (paste, import, upload a zip). */
export default function BankToolbar({ panel }: { panel: SkillBankPanelState }) {
  const { selectedFolder, selectedSkill, addMenuOpen, refusals } = panel;
  const zipInput = useRef<HTMLInputElement>(null);
  return (
    <div className="flex items-center gap-2 px-3 py-2.5">
      <label className="flex min-w-0 flex-1 items-center gap-1.5 rounded-md border border-line-strong bg-surface-2 px-2 py-1.5">
        <Search size={12} className="shrink-0 text-text-3" />
        <input
          value={panel.filter}
          onChange={(event) => panel.setFilter(event.target.value)}
          placeholder="Filter skills…"
          aria-label="Filter skills"
          data-testid="skill-filter"
          className="min-w-0 flex-1 bg-transparent text-text outline-none placeholder:text-text-3 text-xs"
        />
      </label>
      <RefusalTip label="New folder" reason={refusals.createFolder}>
        <button
          type="button"
          onClick={() => {
            if (refusals.createFolder) return;
            void panel.newFolder(selectedFolder?.id ?? (selectedSkill?.folder_id ?? null));
          }}
          aria-label="New folder"
          aria-disabled={refusals.createFolder ? true : undefined}
          title={refusals.createFolder ? undefined : "New folder"}
          data-testid="skill-new-folder"
          className={`flex shrink-0 items-center gap-1 rounded-md border border-line-strong bg-surface-2 px-2 py-1.5 text-text-2 hover:border-primary ${GREYED}`}
        >
          <Folder size={12} />
          <span className="text-sm" style={{ lineHeight: 1 }}>+</span>
        </button>
      </RefusalTip>
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
          className="flex shrink-0 items-center gap-1 rounded-md bg-primary px-2.5 py-1.5 font-medium text-surface-0 hover:opacity-90 text-xs"
        >
          <Plus size={12} />
          Add
          <ChevronDown size={11} />
        </button>
        {addMenuOpen && (
          <div
            role="menu"
            data-testid="skill-add-menu"
            className="absolute right-0 top-8 z-10 w-64 rounded-md border border-line bg-surface-2 p-1 shadow-xl"
            onClick={(event) => event.stopPropagation()}
          >
            <MenuItem
              icon={<ClipboardPaste size={12} />}
              label="Paste SKILL.md…"
              testid="skill-paste"
              disabledReason={refusals.createSkill}
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
              disabledReason={refusals.importFromSource}
              onClick={() => {
                panel.setAddMenuOpen(false);
                panel.setImportOpen(true);
              }}
            />
            <MenuItem
              icon={<FileArchive size={12} />}
              label="Upload a zip…"
              hint=".skills.zip"
              testid="skill-upload-zip"
              disabledReason={refusals.uploadZip}
              onClick={() => zipInput.current?.click()}
            />
            <MenuReasons reasons={reasonsOf(refusals.createSkill, refusals.importFromSource, refusals.uploadZip)} />
          </div>
        )}
        {/* Outside the menu: it stays mounted while the file picker is open. */}
        <input
          ref={zipInput}
          type="file"
          accept=".zip,application/zip"
          className="hidden"
          data-testid="skill-upload-zip-input"
          onChange={(event) => {
            const file = event.target.files?.[0];
            event.target.value = "";
            if (file && !refusals.uploadZip) void panel.uploadZip(file);
          }}
        />
      </span>
    </div>
  );
}
