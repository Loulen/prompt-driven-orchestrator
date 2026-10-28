import type { ReactNode } from "react";
import { fileKey } from "../model/diffTab";
import type { DiffFile } from "../../shared/generated/DiffFile";

// The small pieces of the Diff tab (#748): its empty and loading states, the
// +/- counts and bar, a file's status letter, path and badge.

export function EmptyState({
  testid,
  icon,
  title,
  subtitle,
  action,
}: {
  testid: string;
  icon: ReactNode;
  title: string;
  subtitle: string;
  action?: ReactNode;
}) {
  return (
    <div
      className="flex flex-1 flex-col items-center justify-center gap-2 px-6 py-10 text-center"
      data-testid={testid}
    >
      <span className="grid h-7 w-7 place-items-center rounded border border-line bg-surface-2 text-text-3">
        {icon}
      </span>
      <div className="text-text-3 text-sm">
        {title}
      </div>
      <div className="text-text-3 text-xs">
        {subtitle}
      </div>
      {action}
    </div>
  );
}

export function Skeleton() {
  return (
    <div className="flex flex-col gap-2 px-3 py-3" data-testid="diff-loading" aria-busy>
      <div className="h-3 w-24 animate-pulse rounded bg-surface-2" />
      <div className="mt-1 h-px bg-line" />
      <div className="h-3 w-3/4 animate-pulse rounded bg-surface-2" />
      <div className="h-3 w-full animate-pulse rounded bg-surface-2" />
      <div className="h-3 w-2/3 animate-pulse rounded bg-surface-2" />
    </div>
  );
}

export function Counts({ additions, deletions }: { additions: number; deletions: number }) {
  return (
    <span className="shrink-0 font-mono tabular-nums text-xs">
      <span className="text-st-done">+{additions}</span>{" "}
      <span className="text-st-failed">−{deletions}</span>
    </span>
  );
}

/** Five blocks, green then red, proportional to the file's +/- share. */
export function MiniBar({ additions, deletions }: { additions: number; deletions: number }) {
  const total = additions + deletions;
  const green = total === 0 ? 0 : Math.round((additions / total) * 5);
  return (
    <span className="flex shrink-0 gap-px" aria-hidden>
      {Array.from({ length: 5 }, (_, i) => (
        <span
          key={i}
          className={`h-2 w-1.5 rounded-[1px] ${
            total === 0 ? "bg-surface-2" : i < green ? "bg-st-done" : "bg-st-failed"
          }`}
        />
      ))}
    </span>
  );
}

const STATUS_LETTER: Record<DiffFile["status"], { letter: string; className: string }> = {
  modified: { letter: "M", className: "text-st-await" },
  added: { letter: "A", className: "text-st-done" },
  deleted: { letter: "D", className: "text-st-failed" },
  renamed: { letter: "R", className: "text-st-running" },
  copied: { letter: "C", className: "text-st-running" },
};

export function StatusLetter({ status }: { status: DiffFile["status"] }) {
  const s = STATUS_LETTER[status];
  return (
    <span className={`w-2.5 shrink-0 text-center font-mono font-medium ${s.className}`}>
      {s.letter}
    </span>
  );
}

/** Directory dimmed, file name in clear; `old → new` for a rename/copy. */
export function FilePath({ file }: { file: DiffFile }) {
  const isMove = file.status === "renamed" || file.status === "copied";
  const key = fileKey(file);
  const i = key.lastIndexOf("/");
  const dir = i < 0 ? "" : key.slice(0, i + 1);
  const name = i < 0 ? key : key.slice(i + 1);
  return (
    <span className="min-w-0 truncate font-mono" title={isMove ? `${file.old_path} → ${file.new_path}` : key}>
      {isMove && file.old_path && (
        <>
          <span className="text-text-3">{file.old_path}</span>
          <span className="text-text-3"> → </span>
        </>
      )}
      <span className="text-text-3">{dir}</span>
      <span className="text-text-2">{name || "(unknown path)"}</span>
    </span>
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
  const tone =
    label === "new"
      ? "border-st-done/40 text-st-done"
      : label === "deleted"
        ? "border-st-failed/40 text-st-failed"
        : "border-line-strong text-text-3";
  return (
    <span
      className={`text-2xs shrink-0 rounded border px-1 leading-4 ${tone}`}
      data-testid="diff-file-badge"
    >
      {label}
    </span>
  );
}
