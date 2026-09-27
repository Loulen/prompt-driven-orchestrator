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
        failed ? "border-st-failed/60 bg-st-failed-bg" : status.state === "uploading" ? "border-acc/50 bg-bg-3" : "border-line bg-bg-3"
      }`}
      style={{ fontSize: "11px" }}
      data-testid="paste-skill-file"
      data-path={file.path}
      data-state={status.state}
    >
      <FileText size={11} className={`shrink-0 ${failed ? "text-st-failed" : "text-fg-3"}`} />
      <span className="truncate font-mono text-fg">{file.path}</span>
      <StatusBadge status={status} replaces={file.replaces} />
      {failed && <span className="truncate text-fg-2">{status.message}</span>}
      {status.state === "uploading" && (
        <span className="h-1 w-20 overflow-hidden rounded bg-bg-5" aria-hidden>
          <span className="block h-full w-1/2 animate-pulse bg-acc" />
        </span>
      )}
      <span className="flex-1" />
      {failed ? (
        <>
          <button
            type="button"
            onClick={onRetry}
            data-testid="paste-skill-file-retry"
            className="rounded-md border border-line-strong bg-bg-4 px-2 py-0.5 text-fg-2 hover:border-acc"
            style={{ fontSize: "10.5px" }}
          >
            Retry
          </button>
          <button
            type="button"
            onClick={onSkip}
            data-testid="paste-skill-file-skip"
            className="rounded-md border border-line-strong bg-bg-4 px-2 py-0.5 text-fg-2 hover:border-acc"
            style={{ fontSize: "10.5px" }}
          >
            Skip
          </button>
        </>
      ) : (
        size !== null && (
          <span className="font-mono text-fg-4" style={{ fontSize: "10.5px" }}>
            {formatBytes(size)}
          </span>
        )
      )}
      {editing && (
        <button
          type="button"
          aria-label={`Remove ${file.path}`}
          onClick={onRemove}
          className="grid h-4 w-4 place-items-center rounded text-fg-4 hover:text-fg"
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
        <span className={`${base} border-acc/50 text-acc`} style={style}>
          {replaces ? "replaces" : "to upload"}
        </span>
      );
    case "uploading":
      return (
        <span className={`${base} border-acc/50 text-acc`} style={style}>
          uploading
        </span>
      );
    case "uploaded":
      return (
        <span className={`${base} border-line text-fg-4`} style={style}>
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
        <span className={`${base} border-line text-fg-4`} style={style}>
          skipped
        </span>
      );
  }
}
