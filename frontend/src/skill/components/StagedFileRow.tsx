import { FileText, X } from "lucide-react";
import { formatBytes, type StagedFile } from "../model/skillFiles";

/** A reference file staged in the paste popup: its state (to upload, uploading, failed…) and its actions. */
export default function StagedRow({
  file,
  editing,
  onRemove,
  onRetry,
  onSkip,
}: {
  file: StagedFile;
  editing: boolean;
  onRemove: () => void;
  onRetry: () => void;
  onSkip: () => void;
}) {
  const status = file.status;
  const failed = status.state === "failed";
  const size = status.state === "uploaded" ? status.size : file.size;
  return (
    <li
      className={`flex items-center gap-2 rounded-md border px-3 py-1.5 ${
        failed ? "border-st-failed/60 bg-st-failed/14" : status.state === "uploading" ? "border-primary/50 bg-surface-2" : "border-line bg-surface-2"
      }`}
      style={{ fontSize: "11px" }}
      data-testid="paste-skill-file"
      data-path={file.path}
      data-state={status.state}
    >
      <FileText size={11} className={`shrink-0 ${failed ? "text-st-failed" : "text-text-3"}`} />
      <span className="truncate font-mono text-text">{file.path}</span>
      <StatusBadge status={status} replaces={file.replaces} />
      {failed && <span className="truncate text-text-2">{status.message}</span>}
      {status.state === "uploading" && (
        <span className="h-1 w-20 overflow-hidden rounded bg-surface-2" aria-hidden>
          <span className="block h-full w-1/2 animate-pulse bg-primary" />
        </span>
      )}
      <span className="flex-1" />
      {failed ? (
        <>
          <button
            type="button"
            onClick={onRetry}
            data-testid="paste-skill-file-retry"
            className="rounded-md border border-line-strong bg-surface-2 px-2 py-0.5 text-text-2 hover:border-primary"
            style={{ fontSize: "10.5px" }}
          >
            Retry
          </button>
          <button
            type="button"
            onClick={onSkip}
            data-testid="paste-skill-file-skip"
            className="rounded-md border border-line-strong bg-surface-2 px-2 py-0.5 text-text-2 hover:border-primary"
            style={{ fontSize: "10.5px" }}
          >
            Skip
          </button>
        </>
      ) : (
        size !== null && (
          <span className="font-mono text-text-3" style={{ fontSize: "10.5px" }}>
            {formatBytes(size)}
          </span>
        )
      )}
      {editing && (
        <button
          type="button"
          aria-label={`Remove ${file.path}`}
          onClick={onRemove}
          className="grid h-4 w-4 place-items-center rounded text-text-3 hover:text-text"
        >
          <X size={10} />
        </button>
      )}
    </li>
  );
}

function StatusBadge({ status, replaces }: { status: StagedFile["status"]; replaces?: boolean }) {
  const base = "rounded border px-1.5";
  const style = { fontSize: "9.5px" };
  switch (status.state) {
    case "staged":
      return (
        <span className={`${base} border-primary/50 text-primary`} style={style}>
          {replaces ? "replaces" : "to upload"}
        </span>
      );
    case "uploading":
      return (
        <span className={`${base} border-primary/50 text-primary`} style={style}>
          uploading
        </span>
      );
    case "uploaded":
      return (
        <span className={`${base} border-line text-text-3`} style={style}>
          uploaded
        </span>
      );
    case "failed":
      return (
        <span className={`${base} border-st-failed/60 text-st-failed`} style={style}>
          failed
        </span>
      );
    case "skipped":
      return (
        <span className={`${base} border-line text-text-3`} style={style}>
          skipped
        </span>
      );
  }
}
