import { Folder } from "lucide-react";
import { Tooltip } from "../../shared/ui/kit";
import type { SkillFolder } from "../../shared/generated/SkillFolder";
import { folderPathLabel } from "../model/skillTree";

/** The small controls the bank's panes share: menu rows, action buttons, tabs, the folder picker. */

/** How a gesture the caller's rights refuse looks (CORE-205): greyed, still focusable. */
export const GREYED = "aria-disabled:cursor-not-allowed aria-disabled:opacity-45";

/**
 * A control greyed for `reason` (CORE-205): its tooltip says its name then the
 * refusal's sentence. Without a reason, the control as is.
 */
export function RefusalTip({ label, reason, children }: { label: string; reason?: string; children: React.ReactElement }) {
  return (
    <Tooltip label={label} description={reason} disabled={!reason}>
      {children}
    </Tooltip>
  );
}

export function MenuItem({
  icon,
  label,
  hint,
  danger,
  testid,
  onClick,
  disabledReason,
}: {
  icon: React.ReactNode;
  label: string;
  hint?: string;
  danger?: boolean;
  testid?: string;
  onClick: () => void;
  /** Greyed and inert; the menu says the reason once at its foot (`MenuReasons`). */
  disabledReason?: string;
}) {
  return (
    <button
      type="button"
      role="menuitem"
      data-testid={testid}
      aria-disabled={disabledReason ? true : undefined}
      onClick={disabledReason ? undefined : onClick}
      className={`flex w-full items-center gap-2 rounded px-2 py-1.5 text-left hover:bg-active aria-disabled:hover:bg-transparent ${GREYED} ${
        danger ? "text-st-failed" : "text-text-2"
      } text-xs`}
    >
      <span className="shrink-0">{icon}</span>
      <span className="flex-1 whitespace-nowrap">{label}</span>
      {hint && (
        <span className="font-mono text-text-3 text-2xs">
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
  disabledReason,
}: {
  icon: React.ReactNode;
  label: string;
  onClick: () => void;
  danger?: boolean;
  testid?: string;
  /** Why the caller may not (CORE-205): greyed, inert, its tooltip says the sentence. */
  disabledReason?: string;
}) {
  return (
    <RefusalTip label={label} reason={disabledReason}>
      <button
        type="button"
        onClick={disabledReason ? undefined : onClick}
        aria-disabled={disabledReason ? true : undefined}
        data-testid={testid}
        className={`flex shrink-0 items-center gap-1.5 whitespace-nowrap rounded-md border px-2.5 py-1.5 transition-colors ${GREYED} ${
          danger
            ? "border-st-failed/40 text-st-failed hover:bg-st-failed/14"
            : "border-line-strong bg-surface-2 text-text-2 hover:border-primary"
        } text-xs`}
      >
        {icon}
        {label}
      </button>
    </RefusalTip>
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
      className="absolute left-0 top-9 z-10 w-64 rounded-md border border-line bg-surface-2 p-1 shadow-xl"
      onClick={(event) => event.stopPropagation()}
    >
      <div className="px-2 py-1 text-text-3 uppercase tracking-wide text-2xs">
        Move to
      </div>
      <button
        type="button"
        role="menuitem"
        onClick={() => onPick(null)}
        disabled={current === null}
        className="flex w-full items-center gap-2 rounded px-2 py-1.5 text-left text-text-2 hover:bg-active disabled:opacity-40 text-xs"
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
          className="flex w-full items-center gap-2 rounded px-2 py-1.5 text-left text-text-2 hover:bg-active disabled:opacity-40 text-xs"
        >
          <Folder size={12} className="shrink-0" />
          <span className="truncate">{folderPathLabel(folder.id, folders)}</span>
        </button>
      ))}
      <MenuSeparator />
      <button
        type="button"
        onClick={onClose}
        className="w-full rounded px-2 py-1 text-left text-text-3 hover:bg-active text-xs"
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
      className={`-mb-px border-b-2 px-1 pb-2 ${active ? "border-primary text-text" : "border-transparent text-text-3 hover:text-text"} text-sm`}
    >
      {label}
    </button>
  );
}
