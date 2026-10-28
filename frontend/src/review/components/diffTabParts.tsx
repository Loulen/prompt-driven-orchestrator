import { FileDiff, FileMinus, FilePlus, FileSymlink, type LucideIcon } from "lucide-react";
import { fileKey } from "../model/diffTab";
import type { DiffFile } from "../../shared/generated/DiffFile";

// The small pieces of the Diff tab (#748): its loading state, the +/- counts,
// a file's status (icon and letter), its name and folder, its badge.

export function Skeleton() {
  return (
    <div className="pi-skel" data-testid="diff-loading" aria-busy>
      <span style={{ width: "30%" }} />
      <span style={{ width: "75%" }} />
      <span style={{ width: "100%" }} />
      <span style={{ width: "66%" }} />
    </div>
  );
}

export function Counts({ additions, deletions }: { additions: number; deletions: number }) {
  return (
    <span className="pi-counts">
      <span className="ei-add">+{additions}</span>
      <span className="ei-del">−{deletions}</span>
    </span>
  );
}

const STATUS: Record<DiffFile["status"], { letter: string; color: string; icon: LucideIcon }> = {
  modified: { letter: "M", color: "var(--color-st-await)", icon: FileDiff },
  added: { letter: "A", color: "var(--color-st-done)", icon: FilePlus },
  deleted: { letter: "D", color: "var(--color-st-failed)", icon: FileMinus },
  renamed: { letter: "R", color: "var(--color-st-running)", icon: FileSymlink },
  copied: { letter: "C", color: "var(--color-st-running)", icon: FileSymlink },
};

/** The status icon of a file, in its status colour. */
export function StatusIcon({ status }: { status: DiffFile["status"] }) {
  const s = STATUS[status];
  return <s.icon size={14} strokeWidth={1.5} color={s.color} aria-hidden />;
}

/** The status letter at the end of a file row (`M`, `A`, `D`, `R`, `C`). */
export function StatusLetter({ status }: { status: DiffFile["status"] }) {
  const s = STATUS[status];
  return (
    <span className="pi-dfile__st" style={{ color: s.color }} title={status}>
      {s.letter}
    </span>
  );
}

/** The characters a long file name keeps at its end when it is cut. */
const NAME_TAIL = 10;

/**
 * A file name cut in its middle: long names often differ only at their end
 * (`a_very_long_new_file_3.ts`), so the end and the extension stay visible.
 */
function FileName({ name }: { name: string }) {
  const cut = name.length > NAME_TAIL * 2 ? name.length - NAME_TAIL : name.length;
  return (
    <span className="pi-dfile__name" title={name}>
      <span className="pi-dfile__head">{name.slice(0, cut)}</span>
      {cut < name.length && <span className="pi-dfile__tail">{name.slice(cut)}</span>}
    </span>
  );
}

/** The file name in clear, its folder dimmed after it; `old → new` for a rename/copy. */
export function FilePath({ file }: { file: DiffFile }) {
  const isMove = file.status === "renamed" || file.status === "copied";
  const key = fileKey(file);
  const i = key.lastIndexOf("/");
  const dir = i < 0 ? "" : key.slice(0, i + 1);
  const name = i < 0 ? key : key.slice(i + 1);
  return (
    <>
      <FileName name={name || "(unknown path)"} />
      <span className="pi-dfile__dir d-mono">
        {isMove && file.old_path ? `${file.old_path} → ${dir}` : dir}
      </span>
    </>
  );
}

export function Badge({ file }: { file: DiffFile }) {
  const label = file.binary
    ? "binary"
    : file.status === "added"
      ? "new"
      : file.status === "deleted"
        ? "deleted"
        : file.status === "renamed"
          ? "renamed"
          : file.status === "copied"
            ? "copied"
            : null;
  if (!label) return null;
  return (
    <span className="d-chip" data-testid="diff-file-badge">
      {label}
    </span>
  );
}
