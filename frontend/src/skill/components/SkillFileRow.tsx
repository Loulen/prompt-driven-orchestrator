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
        confirming ? "border-st-failed/60 bg-st-failed-bg" : active ? "border-acc/60 bg-bg-3" : badge ? "border-acc/40 bg-bg-3" : "border-line bg-bg-3"
      }`}
      style={{ fontSize: "11.5px" }}
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
        <FileText size={12} className="shrink-0 text-fg-3" />
        <span className="truncate font-mono text-fg">{path}</span>
        {badge && (
          <>
            <span className="shrink-0 rounded border border-acc/50 px-1.5 text-acc" style={{ fontSize: "9.5px" }}>
              {badge.text}
            </span>
            {badge.detail && !compact && (
              <span className="truncate text-fg-4" style={{ fontSize: "10.5px" }}>
                {badge.detail}
              </span>
            )}
          </>
        )}
      </button>
      {confirming ? (
        <span className="flex items-center gap-1.5" data-testid="skill-file-delete-confirm">
          <span className="text-fg-2" style={{ fontSize: "11px" }}>
            Delete this file?
          </span>
          <button
            type="button"
            onClick={onKeep}
            className="rounded-md border border-line-strong bg-bg-4 px-2 py-0.5 text-fg-2 hover:border-acc"
            style={{ fontSize: "10.5px" }}
          >
            Keep
          </button>
          <button
            type="button"
            onClick={onDelete}
            data-testid="skill-file-delete-yes"
            className="rounded-md bg-st-failed px-2 py-0.5 font-medium text-white hover:opacity-90"
            style={{ fontSize: "10.5px" }}
          >
            Delete
          </button>
        </span>
      ) : (
        <>
          {size !== null && (
            <span className="shrink-0 font-mono text-fg-4" style={{ fontSize: "10.5px" }}>
              {formatBytes(size)}
            </span>
          )}
          <button
            type="button"
            onClick={onOpen}
            aria-label={`Edit ${path} as plain text`}
            className="grid h-5 w-5 shrink-0 place-items-center rounded text-fg-4 hover:bg-bg-4 hover:text-fg"
          >
            <Pencil size={11} />
          </button>
          {deletable && (
            <button
              type="button"
              onClick={onAskDelete}
              aria-label={`Delete ${path}`}
              data-testid="skill-file-delete"
              className="grid h-5 w-5 shrink-0 place-items-center rounded text-fg-4 hover:bg-bg-4 hover:text-st-failed"
            >
              <Trash2 size={11} />
            </button>
          )}
        </>
      )}
    </div>
  );
}
