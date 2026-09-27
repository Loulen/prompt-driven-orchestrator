import { Folder } from "lucide-react";
import type { SkillFolder } from "../../shared/generated/SkillFolder";
import { folderPathLabel } from "../model/skillTree";

/** The small controls the bank's panes share: menu rows, action buttons, tabs, the folder picker. */

export function MenuItem({
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
    <button
      type="button"
      role="menuitem"
      data-testid={testid}
      onClick={onClick}
      className={`flex w-full items-center gap-2 rounded px-2 py-1.5 text-left hover:bg-bg-5 ${
        danger ? "text-st-failed" : "text-fg-2"
      }`}
      style={{ fontSize: "11.5px" }}
    >
      <span className="shrink-0">{icon}</span>
      <span className="flex-1 whitespace-nowrap">{label}</span>
      {hint && (
        <span className="font-mono text-fg-4" style={{ fontSize: "10px" }}>
          {hint}
        </span>
      )}
    </button>
  );
}

export function MenuSeparator() {
  return <div className="my-1 border-t border-line" />;
}

export function ActionButton({
  icon,
  label,
  onClick,
  danger,
  testid,
}: {
  icon: React.ReactNode;
  label: string;
  onClick: () => void;
  danger?: boolean;
  testid?: string;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      data-testid={testid}
      className={`flex shrink-0 items-center gap-1.5 whitespace-nowrap rounded-md border px-2.5 py-1.5 transition-colors ${
        danger
          ? "border-st-failed/40 text-st-failed hover:bg-st-failed-bg"
          : "border-line-strong bg-bg-3 text-fg-2 hover:border-acc"
      }`}
      style={{ fontSize: "11.5px" }}
    >
      {icon}
      {label}
    </button>
  );
}

export function FolderPicker({
  folders,
  exclude,
  current,
  onPick,
  onClose,
}: {
  folders: SkillFolder[];
  /** Folder ids not offered (a folder cannot move under itself). */
  exclude?: Set<string>;
  current: string | null;
  onPick: (folderId: string | null) => void;
  onClose: () => void;
}) {
  const options = folders.filter((folder) => !exclude?.has(folder.id));
  return (
    <div
      role="menu"
      data-testid="move-picker"
      className="absolute left-0 top-9 z-10 w-64 rounded-md border border-line bg-bg-4 p-1 shadow-xl"
      onClick={(event) => event.stopPropagation()}
    >
      <div className="px-2 py-1 text-fg-4 uppercase tracking-wide" style={{ fontSize: "9.5px" }}>
        Move to
      </div>
      <button
        type="button"
        role="menuitem"
        onClick={() => onPick(null)}
        disabled={current === null}
        className="flex w-full items-center gap-2 rounded px-2 py-1.5 text-left text-fg-2 hover:bg-bg-5 disabled:opacity-40"
        style={{ fontSize: "11.5px" }}
      >
        <Folder size={12} /> Root of the bank
      </button>
      {options.map((folder) => (
        <button
          key={folder.id}
          type="button"
          role="menuitem"
          onClick={() => onPick(folder.id)}
          disabled={current === folder.id}
          className="flex w-full items-center gap-2 rounded px-2 py-1.5 text-left text-fg-2 hover:bg-bg-5 disabled:opacity-40"
          style={{ fontSize: "11.5px" }}
        >
          <Folder size={12} className="shrink-0" />
          <span className="truncate">{folderPathLabel(folder.id, folders)}</span>
        </button>
      ))}
      <MenuSeparator />
      <button
        type="button"
        onClick={onClose}
        className="w-full rounded px-2 py-1 text-left text-fg-4 hover:bg-bg-5"
        style={{ fontSize: "11px" }}
      >
        Cancel
      </button>
    </div>
  );
}

export function TabButton({ active, onClick, label }: { active: boolean; onClick: () => void; label: React.ReactNode }) {
  return (
    <button
      type="button"
      role="tab"
      aria-selected={active}
      onClick={onClick}
      className={`-mb-px border-b-2 px-1 pb-2 ${active ? "border-acc text-fg" : "border-transparent text-fg-3 hover:text-fg"}`}
      style={{ fontSize: "12px" }}
    >
      {label}
    </button>
  );
}
