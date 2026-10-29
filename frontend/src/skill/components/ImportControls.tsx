import { Folder } from "lucide-react";
import type { SkillCandidate } from "../../shared/generated/SkillCandidate";
import type { SkillFolder } from "../../shared/generated/SkillFolder";
import { folderPathLabel } from "../model/skillTree";

/** The small pieces the import popup's sections share: chips, badges, summary rows, the folder select. */

export function Chip({ label, value, accent }: { label: string; value: string; accent?: boolean }) {
  return (
    <span
      className={`flex items-center gap-1 rounded border px-1.5 py-0.5 ${accent ? "border-primary/40 bg-primary/10" : "border-line bg-surface-2"} text-xs`}
    >
      <span className="text-text-3">{label}</span>
      <span className="font-mono text-text">{value}</span>
    </span>
  );
}

export function StatusBadge({ candidate }: { candidate: SkillCandidate }) {
  const base = "rounded border px-1.5 py-0.5 whitespace-nowrap text-2xs";
  switch (candidate.status) {
    case "new":
      return (
        <span className={`${base} border-primary/40 bg-primary/10 text-primary`}>
          new
        </span>
      );
    case "name_taken":
      return (
        <span className={`${base} border-st-blocked/50 bg-st-blocked/14 text-st-blocked`}>
          name taken{candidate.existing?.folder_name ? ` · in “${candidate.existing.folder_name}”` : ""}
        </span>
      );
    case "same_commit":
      return (
        <span className={`${base} border-line bg-surface-2 text-text-3`}>
          same commit{candidate.existing?.folder_name ? ` · already in “${candidate.existing.folder_name}”` : " · already in bank"}
        </span>
      );
    default:
      return (
        <span className={`${base} border-st-failed/40 bg-st-failed/14 text-st-failed`}>
          not importable
        </span>
      );
  }
}

export function SummaryRow({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex gap-3 border-t border-line px-3 py-1.5 first:border-t-0">
      <span className="w-[84px] shrink-0 text-text-3">{label}</span>
      <span className="min-w-0 flex-1 text-text-2">{children}</span>
    </div>
  );
}

export function ParentSelect({
  folders,
  value,
  onChange,
  disabled,
  prefix,
}: {
  folders: SkillFolder[];
  value: string | null;
  onChange: (id: string | null) => void;
  disabled?: boolean;
  prefix?: string;
}) {
  return (
    <label className="flex items-center gap-2 rounded-md border border-line-strong bg-surface-2 px-2.5 py-1.5">
      {prefix ? (
        <span className="text-text-3 text-xs">
          {prefix}
        </span>
      ) : (
        <Folder size={12} className="shrink-0 text-st-await" />
      )}
      <select
        value={value ?? ""}
        onChange={(event) => onChange(event.target.value || null)}
        disabled={disabled}
        aria-label="Into folder"
        data-testid="import-parent-folder"
        className="w-full bg-transparent text-text outline-none disabled:opacity-60 text-xs"
      >
        <option value="">Root of the bank</option>
        {folders.map((folder) => (
          <option key={folder.id} value={folder.id}>
            {folderPathLabel(folder.id, folders)}
          </option>
        ))}
      </select>
    </label>
  );
}
