import { Folder } from "lucide-react";
import type { SkillCandidate } from "../../shared/generated/SkillCandidate";
import type { SkillFolder } from "../../shared/generated/SkillFolder";
import { folderPathLabel } from "../model/skillTree";

/** The small pieces the import popup's sections share: chips, badges, summary rows, the folder select. */

export function Chip({ label, value, accent }: { label: string; value: string; accent?: boolean }) {
  return (
    <span
      className={`flex items-center gap-1 rounded border px-1.5 py-0.5 ${accent ? "border-acc/40 bg-acc/10" : "border-line bg-bg-3"}`}
      style={{ fontSize: "10.5px" }}
    >
      <span className="text-fg-4">{label}</span>
      <span className="font-mono text-fg">{value}</span>
    </span>
  );
}

export function StatusBadge({ candidate }: { candidate: SkillCandidate }) {
  const base = "rounded border px-1.5 py-0.5 whitespace-nowrap";
  const size = { fontSize: "9.5px" };
  switch (candidate.status) {
    case "new":
      return (
        <span className={`${base} border-acc/40 bg-acc/10 text-acc`} style={size}>
          new
        </span>
      );
    case "name_taken":
      return (
        <span className={`${base} border-st-blocked/50 bg-st-blocked-bg text-st-blocked`} style={size}>
          name taken{candidate.existing?.folder_name ? ` · in “${candidate.existing.folder_name}”` : ""}
        </span>
      );
    case "same_commit":
      return (
        <span className={`${base} border-line bg-bg-3 text-fg-4`} style={size}>
          same commit{candidate.existing?.folder_name ? ` · already in “${candidate.existing.folder_name}”` : " · already in bank"}
        </span>
      );
    default:
      return (
        <span className={`${base} border-st-failed/40 bg-st-failed-bg text-st-failed`} style={size}>
          not importable
        </span>
      );
  }
}

export function SummaryRow({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex gap-3 border-t border-line px-3 py-1.5 first:border-t-0">
      <span className="w-[84px] shrink-0 text-fg-4">{label}</span>
      <span className="min-w-0 flex-1 text-fg-2">{children}</span>
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
    <label className="flex items-center gap-2 rounded-md border border-line-strong bg-bg-3 px-2.5 py-1.5">
      {prefix ? (
        <span className="text-fg-4" style={{ fontSize: "10.5px" }}>
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
        className="w-full bg-transparent text-fg outline-none disabled:opacity-60"
        style={{ fontSize: "11.5px" }}
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
