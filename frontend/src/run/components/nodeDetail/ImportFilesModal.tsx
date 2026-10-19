import { useCallback, useEffect, useRef, useState } from "react";
import { AlertCircle, FileUp, Send, X } from "lucide-react";
import { importNodeFiles } from "../../api/nodes";
import { sendTextToNodeTerminal } from "../../../session/api/sessions";
import type { ImportedFileEntry } from "../../../shared/generated/ImportedFileEntry";
import {
  attachmentBadge,
  DEFAULT_MAX_ATTACHMENTS_MB,
  formatAttachmentSize,
} from "../../model/attachments";
import { importedFilesText, notPilotRefusal, stageImportFiles } from "../../model/nodeImport";
import { requestTakeControl, TAKE_CONTROL_LABEL } from "../../../session/model/terminalRole";
import ActorLabel from "../../../shared/actors/ActorLabel";
import { showToast } from "../../../shared/ui/toast";
import { copyToClipboard } from "../../../shared/lib/clipboard";
import { useSettings } from "../../../settings/store/useSettings";
import { useFileDropTarget } from "../../../skill/store/useFileDropTarget";
import { DropOverlay } from "../../../skill/components/SkillFileDropZone";
import ImportedText from "./ImportedText";

interface Props {
  runId: string;
  nodeId: string;
  /** The node's current iteration: the one whose session receives the files. */
  iter: number;
  /** The tmux session of the terminal: where « Take control » goes when the
   *  daemon refuses the send to a spectator. */
  session: string;
  /** Files dropped on the terminal; empty when opened from the Import button,
   *  which then opens the file picker straight away. */
  initialFiles: File[];
  /** Set when the node cannot receive files (no live session): the modal says
   *  why and Import stays off. The daemon refuses the same cases anyway. */
  blockedReason?: string | null;
  /** Set when this browser only watches the terminal (ADR-0075: another
   *  browser has control): said beside « Copy & send to terminal ». The daemon
   *  decides: a refused send writes nothing and says who pilots, in a toast
   *  offering to take control. */
  sendBlockedReason?: string | null;
  /** Closes the modal: Cancel, Done, Escape, the backdrop, or once the text
   *  went into the terminal. */
  onClose: () => void;
}

/** No confirmation once sent: the modal closes on the user's explicit gesture;
 *  only a failure is said. */
type SendState =
  | { kind: "idle" }
  | { kind: "sending" }
  | { kind: "failed"; message: string; copied: boolean };

/**
 * « Fichier importé en cours de Run »: the files a user gives a node with a live
 * session, from its terminal (drop or Import button). Two steps:
 *
 * 1. **Select**: the dropped or picked files, their size against the instance
 *    budget (`max_attachments_mb`, per import), Import. A refusal (budget, no
 *    live session) is shown in place, and nothing is written.
 * 2. **Imported**: the ready-to-use text naming each file's final path
 *    (suffix included) relative to the worktree root, a copy icon, and « Copy &
 *    send to terminal », which copies the text, writes it into the agent's input
 *    **without pressing Enter** and closes the modal. PDO never tells the agent
 *    by itself.
 */
export default function ImportFilesModal({
  runId,
  nodeId,
  iter,
  session,
  initialFiles,
  blockedReason,
  sendBlockedReason,
  onClose,
}: Props) {
  const [staged, setStaged] = useState<File[]>(initialFiles);
  const [importing, setImporting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [imported, setImported] = useState<ImportedFileEntry[] | null>(null);
  const [send, setSend] = useState<SendState>({ kind: "idle" });
  const inputRef = useRef<HTMLInputElement>(null);

  const { settings } = useSettings(true);
  const maxMb = settings?.max_attachments_mb?.effective ?? DEFAULT_MAX_ATTACHMENTS_MB;
  const total = staged.reduce((sum, f) => sum + f.size, 0);
  const overBudget = total > maxMb * 1024 * 1024;

  // Opened from the Import button: straight to the picker (the click that
  // opened the modal is the user activation the picker needs).
  const pickOnOpen = useRef(initialFiles.length === 0 && !blockedReason);
  useEffect(() => {
    if (pickOnOpen.current) {
      pickOnOpen.current = false;
      inputRef.current?.click();
    }
  }, []);

  const addFiles = useCallback(
    (files: ArrayLike<File>) => {
      if (imported) return;
      setStaged((prev) => stageImportFiles(prev, files));
      setError(null);
    },
    [imported],
  );
  const onDrop = useCallback((dt: DataTransfer) => addFiles(dt.files), [addFiles]);
  const { dragging, handlers } = useFileDropTarget(onDrop);
  const droppable = dragging !== null && imported === null;

  // The modal owns Escape while it is open: capture phase, before the enlarged
  // terminal's own handler.
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || importing) return;
      event.stopImmediatePropagation();
      event.preventDefault();
      onClose();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [importing, onClose]);

  const canImport =
    !blockedReason && !importing && staged.length > 0 && !overBudget && imported === null;

  const runImport = async () => {
    if (!canImport) return;
    setImporting(true);
    setError(null);
    try {
      const res = await importNodeFiles(runId, nodeId, iter, staged);
      setImported(res.files);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Import failed");
    } finally {
      setImporting(false);
    }
  };

  const paths = imported ? imported.map((f) => f.path) : [];
  const text = importedFilesText(paths);


  const copyAndSend = async () => {
    setSend({ kind: "sending" });
    // Copy first: it is the fallback when the terminal write does not land.
    const copied = await copyToClipboard(text);
    try {
      await sendTextToNodeTerminal(runId, nodeId, iter, text);
      onClose();
    } catch (cause) {
      const refused = notPilotRefusal(cause);
      if (refused) {
        // Another browser pilots: nothing was written. The modal stays, to
        // send again once this browser has taken control.
        setSend({ kind: "idle" });
        showToast({
          kind: "warning",
          title: `Not sent to ${nodeId}`,
          message: (
            <>
              {refused.pilot ? <ActorLabel actor={refused.pilot} /> : "Another browser"} pilots this terminal. Take
              control to write into the agent's input.
            </>
          ),
          action: { label: TAKE_CONTROL_LABEL, run: () => requestTakeControl(session) },
        });
        return;
      }
      const message = cause instanceof Error ? cause.message : "Send failed";
      setSend({ kind: "failed", message, copied });
    }
  };

  return (
    <div
      className="fixed inset-0 z-[60] flex items-center justify-center bg-black/50"
      onClick={(event) => {
        event.stopPropagation();
        if (!importing) onClose();
      }}
      data-testid="import-files-backdrop"
    >
      <div
        className={`relative flex max-h-[86vh] w-[560px] max-w-[94vw] flex-col rounded-lg border bg-bg-4 shadow-xl ${
          droppable ? "border-acc" : "border-line"
        }`}
        onClick={(event) => event.stopPropagation()}
        role="dialog"
        aria-label="Import files into this node"
        aria-busy={importing || undefined}
        data-testid="import-files-modal"
        {...(imported === null ? handlers : {})}
      >
        {droppable && (
          <DropOverlay
            count={dragging}
            title={`Drop to add ${dragging} file${dragging === 1 ? "" : "s"}`}
            hint="Nothing is written before Import"
          />
        )}
        <div className="flex items-center justify-between border-b border-line px-4 py-3">
          <h3 className="flex items-center gap-2 font-semibold text-fg" style={{ fontSize: "13.5px" }}>
            <FileUp size={14} className="text-fg-3" />
            Import files into <span className="font-mono">{nodeId}</span>
          </h3>
          <button
            type="button"
            onClick={onClose}
            disabled={importing}
            aria-label="Close import"
            className="grid h-6 w-6 place-items-center rounded text-fg-3 hover:bg-bg-5 hover:text-fg disabled:opacity-30"
          >
            <X size={14} />
          </button>
        </div>

        <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto p-4" style={{ fontSize: "11.5px" }}>
          {blockedReason && (
            <div
              className="flex items-start gap-2 rounded-md border border-st-failed/50 bg-st-failed-bg px-3 py-2 text-st-failed"
              role="alert"
              data-testid="import-blocked"
            >
              <AlertCircle size={14} className="mt-px shrink-0" />
              <span>{blockedReason}</span>
            </div>
          )}

          {imported === null ? (
            <>
              <p className="text-fg-3">Never committed, removed with the Run.</p>
              <div className="flex items-center gap-2">
                <button
                  type="button"
                  onClick={() => inputRef.current?.click()}
                  disabled={importing || !!blockedReason}
                  className="rounded-md border border-line-strong bg-bg-3 px-2.5 py-1 text-fg-2 hover:bg-bg-5 hover:text-fg disabled:opacity-40"
                  data-testid="import-choose"
                >
                  Choose files…
                </button>
                <span className="text-fg-4">or drop them here</span>
                <span className="flex-1" />
                {staged.length > 0 && (
                  <span className={overBudget ? "text-st-failed" : "text-fg-4"} data-testid="import-counter">
                    {formatAttachmentSize(total)} / {maxMb.toFixed(1)} MB
                  </span>
                )}
                <input
                  ref={inputRef}
                  type="file"
                  multiple
                  className="hidden"
                  data-testid="import-input"
                  onChange={(e) => {
                    if (e.target.files) addFiles(e.target.files);
                    e.target.value = "";
                  }}
                />
              </div>
              {staged.length > 0 && (
                <ul className="flex flex-col gap-1" data-testid="import-files">
                  {staged.map((file) => (
                    <li
                      key={file.name}
                      className="flex items-center gap-2 rounded-md border border-line bg-bg-3 px-2.5 py-1.5"
                    >
                      <span className="rounded border border-line-strong px-1 font-mono text-fg-3" style={{ fontSize: "9px" }}>
                        {attachmentBadge(file.name)}
                      </span>
                      <span className="min-w-0 flex-1 truncate text-fg">{file.name}</span>
                      <span className="text-fg-4">{formatAttachmentSize(file.size)}</span>
                      <button
                        type="button"
                        onClick={() => setStaged((prev) => prev.filter((f) => f !== file))}
                        disabled={importing}
                        aria-label={`Remove ${file.name}`}
                        className="grid h-5 w-5 place-items-center rounded text-fg-4 hover:bg-bg-5 hover:text-fg"
                      >
                        <X size={12} />
                      </button>
                    </li>
                  ))}
                </ul>
              )}
              {overBudget && (
                <p className="text-st-failed" role="alert" data-testid="import-over-budget">
                  {formatAttachmentSize(total)} is over the limit of {maxMb.toFixed(1)} MB per import.
                  Raise <code className="font-mono">max_attachments_mb</code> in Settings or import less.
                </p>
              )}
              {error && (
                <div
                  className="flex items-start gap-2 rounded-md border border-st-failed/50 bg-st-failed-bg px-3 py-2 text-st-failed"
                  role="alert"
                  data-testid="import-error"
                >
                  <AlertCircle size={14} className="mt-px shrink-0" />
                  <span>{error}</span>
                </div>
              )}
            </>
          ) : (
            <>
              <p className="text-fg-2" data-testid="import-done">
                {imported.length === 1 ? "1 file imported." : `${imported.length} files imported.`} Give the agent
                this text:
              </p>
              <ImportedText paths={paths} text={text} />
              {sendBlockedReason && (
                <p className="text-fg-3" data-testid="import-send-blocked">
                  {sendBlockedReason}
                </p>
              )}
              {send.kind === "failed" && (
                <p className="text-st-failed" role="alert" data-testid="import-send-failed">
                  Could not write into the terminal ({send.message}).
                  {send.copied ? " The text is in your clipboard: paste it in the terminal." : ""}
                </p>
              )}
            </>
          )}
        </div>

        <div className="flex items-center justify-end gap-2 border-t border-line px-4 py-3">
          {imported === null ? (
            <>
              <button
                type="button"
                onClick={onClose}
                disabled={importing}
                className="rounded-md px-3 py-1.5 text-fg-3 hover:bg-bg-5 hover:text-fg"
                style={{ fontSize: "12px" }}
              >
                Cancel
              </button>
              <button
                type="button"
                onClick={() => void runImport()}
                disabled={!canImport}
                className="rounded-md bg-acc px-3 py-1.5 font-medium text-on-acc hover:opacity-90 disabled:opacity-40"
                style={{ fontSize: "12px" }}
                data-testid="import-submit"
              >
                {importing ? "Importing…" : staged.length > 1 ? `Import ${staged.length} files` : "Import"}
              </button>
            </>
          ) : (
            <>
              <button
                type="button"
                onClick={onClose}
                className="rounded-md px-3 py-1.5 text-fg-3 hover:bg-bg-5 hover:text-fg"
                style={{ fontSize: "12px" }}
                data-testid="import-close"
              >
                Done
              </button>
              <button
                type="button"
                onClick={() => void copyAndSend()}
                disabled={send.kind === "sending"}
                title={
                  sendBlockedReason ??
                  "Writes the text into the agent's input without submitting it, and copies it"
                }
                className="flex items-center gap-1.5 rounded-md bg-acc px-3 py-1.5 font-medium text-on-acc hover:opacity-90 disabled:opacity-40"
                style={{ fontSize: "12px" }}
                data-testid="import-send"
              >
                <Send size={12} />
                Copy &amp; send to terminal
              </button>
            </>
          )}
        </div>
      </div>
    </div>
  );
}

