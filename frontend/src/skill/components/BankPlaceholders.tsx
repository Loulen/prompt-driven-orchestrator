import { ClipboardPaste, Download, FileText } from "lucide-react";
import type { SkillFolder } from "../../shared/generated/SkillFolder";
import { displaySourceUrl } from "../model/skillSource";

/** What the bank's right pane shows when there is nothing to read yet. */

export function EmptyBank({ onPaste, onImport }: { onPaste: () => void; onImport: () => void }) {
  return (
    <div className="flex flex-1 flex-col items-center justify-center gap-3 p-8 text-center" data-testid="skill-bank-empty">
      <div className="grid h-14 w-14 place-items-center rounded-xl border border-dashed border-line-strong text-text-3">
        <FileText size={20} />
      </div>
      <h3 className="font-semibold text-text" style={{ fontSize: "14px" }}>
        No skills yet
      </h3>
      <p className="max-w-[420px] text-text-3" style={{ fontSize: "12px" }}>
        Import a repo of skills, or paste the text of a <span className="font-mono">SKILL.md</span>. Skills are
        delivered to every worktree PDO creates, never committed.
      </p>
      <div className="mt-1 flex items-center gap-2">
        <button
          type="button"
          onClick={onImport}
          data-testid="skill-import-empty"
          className="flex items-center gap-1.5 rounded-md bg-primary px-3 py-2 font-medium text-surface-0 hover:opacity-90"
          style={{ fontSize: "12px" }}
        >
          <Download size={13} />
          Import from a source
        </button>
        <button
          type="button"
          onClick={onPaste}
          data-testid="skill-paste-empty"
          className="flex items-center gap-1.5 rounded-md border border-line-strong bg-surface-2 px-3 py-2 font-medium text-text-2 hover:border-primary"
          style={{ fontSize: "12px" }}
        >
          <ClipboardPaste size={13} />
          Paste SKILL.md
        </button>
      </div>
    </div>
  );
}

export function RescanInProgress({ folder, onCancel }: { folder: SkillFolder; onCancel: () => void }) {
  const source = folder.source;
  return (
    <div className="flex min-h-0 flex-1 flex-col p-5" data-testid="folder-rescan">
      <h3 className="font-semibold text-text" style={{ fontSize: "17px" }}>
        Update {folder.name} from its source?
      </h3>
      <p className="mt-2 text-text-3" style={{ fontSize: "12px" }}>
        Re-scanning <span className="font-mono">{source ? displaySourceUrl(source.url) : ""}</span>
        {source?.ref ? <span className="font-mono">@{source.ref}</span> : null}… nothing is written before you confirm.
      </p>
      <div className="mt-6 flex flex-col items-center gap-3">
        <div className="h-1 w-[280px] overflow-hidden rounded-full bg-surface-2">
          <div className="h-full w-1/3 animate-pulse rounded-full bg-st-running" />
        </div>
        <span className="text-text-3" style={{ fontSize: "10.5px" }}>
          Shallow clone with your git credentials
        </span>
        <button
          type="button"
          onClick={onCancel}
          className="rounded-md border border-line-strong bg-surface-2 px-3 py-1.5 text-text-2 hover:bg-hover"
          style={{ fontSize: "11.5px" }}
        >
          Cancel
        </button>
      </div>
    </div>
  );
}
