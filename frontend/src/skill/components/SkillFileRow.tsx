import { FileText, Pencil, Trash2 } from "lucide-react";
import { formatBytes } from "../model/skillFiles";

/** One row of a skill's Files tab: open it in the editor, or delete it after an inline Keep / Delete. */
export default function FileRow({
  path,
  size,
  active,
  badge,
  compact,
  deletable,
  confirming,
  onOpen,
  onAskDelete,
  onKeep,
  onDelete,
}: {
  path: string;
  size: number | null;
  active: boolean;
  badge: { text: string; tone: "acc"; detail?: string } | null;
  compact: boolean;
  deletable: boolean;
  confirming: boolean;
  onOpen: () => void;
  onAskDelete: () => void;
  onKeep: () => void;
  onDelete: () => void;
}) {
  return (
    <div
      className={`flex items-center gap-2 rounded-md border px-3 py-1.5 ${
        confirming ? "border-st-failed/60 bg-st-failed/14" : active ? "border-primary/60 bg-surface-2" : badge ? "border-primary/40 bg-surface-2" : "border-line bg-surface-2"
      } text-xs`}
      data-testid="skill-file-row"
      data-path={path}
    >
      <button
        type="button"
        onClick={onOpen}
        className="flex min-w-0 flex-1 items-center gap-2 text-left"
        aria-label={`Edit ${path}`}
        data-testid="skill-file-open"
      >
        <FileText size={12} className="shrink-0 text-text-3" />
        <span className="truncate font-mono text-text">{path}</span>
        {badge && (
          <>
            <span className="shrink-0 rounded border border-primary/50 px-1.5 text-primary text-2xs">
              {badge.text}
            </span>
            {badge.detail && !compact && (
              <span className="truncate text-text-3 text-xs">
                {badge.detail}
              </span>
            )}
          </>
        )}
      </button>
      {confirming ? (
        <span className="flex items-center gap-1.5" data-testid="skill-file-delete-confirm">
          <span className="text-text-2 text-xs">
            Delete this file?
          </span>
          <button
            type="button"
            onClick={onKeep}
            className="rounded-md border border-line-strong bg-surface-2 px-2 py-0.5 text-text-2 hover:border-primary text-xs"
          >
            Keep
          </button>
          <button
            type="button"
            onClick={onDelete}
            data-testid="skill-file-delete-yes"
            className="rounded-md bg-st-failed px-2 py-0.5 font-medium text-primary-ink hover:opacity-90 text-xs"
          >
            Delete
          </button>
        </span>
      ) : (
        <>
          {size !== null && (
            <span className="shrink-0 font-mono text-text-3 text-xs">
              {formatBytes(size)}
            </span>
          )}
          <button
            type="button"
            onClick={onOpen}
            aria-label={`Edit ${path} as plain text`}
            className="grid h-5 w-5 shrink-0 place-items-center rounded text-text-3 hover:bg-hover hover:text-text"
          >
            <Pencil size={11} />
          </button>
          {deletable && (
            <button
              type="button"
              onClick={onAskDelete}
              aria-label={`Delete ${path}`}
              data-testid="skill-file-delete"
              className="grid h-5 w-5 shrink-0 place-items-center rounded text-text-3 hover:bg-hover hover:text-st-failed"
            >
              <Trash2 size={11} />
            </button>
          )}
        </>
      )}
    </div>
  );
}
