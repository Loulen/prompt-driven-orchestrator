import { useId, useState } from "react";
import { ConfirmDialog, TextInput } from "../../shared/ui/kit";
import type { CascadeImpact } from "../model/runTree";

interface Props {
  runId: string;
  /** The archive itself: the dialog waits for it, closes on success, keeps a failure on screen. */
  onConfirm: () => Promise<unknown>;
  onCancel: () => void;
  isLive?: boolean;
  /** #815 — what the daemon's cascade drags along (finished children archived
   *  too; live children make the daemon refuse). Absent = childless. */
  cascade?: CascadeImpact;
}

/** The cascade sentence for the dialog body, or null when there is nothing to say. */
function cascadeCopy(cascade: CascadeImpact | undefined): string | null {
  if (!cascade || (cascade.finished === 0 && cascade.live === 0)) return null;
  const parts: string[] = [];
  if (cascade.finished > 0) {
    parts.push(
      `${cascade.finished} finished child run${cascade.finished === 1 ? "" : "s"} will be archived with it.`,
    );
  }
  if (cascade.live > 0) {
    parts.push(
      `${cascade.live} child run${cascade.live === 1 ? " is" : "s are"} still live: the daemon will refuse until ${cascade.live === 1 ? "it is" : "they are"} stopped.`,
    );
  }
  return parts.join(" ");
}

/**
 * The confirmation of a Run's archive, on the kit's `ConfirmDialog`: busy
 * while the daemon archives, closed on success, the daemon's message kept on
 * a failure. A live Run asks for its id's 7-hex suffix first, so the user
 * proves they archive the Run they think they do. #315/ADR-0020: completed
 * outputs are kept on archive, so the copy never claims they are lost.
 */
export default function CleanupConfirmModal({
  runId,
  onConfirm,
  onCancel,
  isLive = false,
  cascade,
}: Props) {
  const cascadeText = cascadeCopy(cascade);
  const shortId = runId.slice(-7);
  const [confirmText, setConfirmText] = useState("");
  const inputId = useId();
  const canConfirm = !isLive || confirmText.trim() === shortId;

  return (
    <ConfirmDialog
      title={isLive ? "Archive this run before it completes?" : "Archive this run?"}
      consequence={isLive ? "Kills its active sessions" : "Removes its worktrees, keeps its outputs"}
      target={runId}
      actionLabel="Archive"
      pendingLabel="Archiving…"
      failedTitle="Archiving failed"
      danger
      confirmDisabled={!canConfirm}
      onConfirm={onConfirm}
      onClose={onCancel}
      data-testid="cleanup-confirm-modal"
      confirmTestId="cleanup-confirm-button"
    >
      {cascadeText && (
        <p className="d-confirm__line" data-testid="cleanup-cascade">
          {cascadeText}
        </p>
      )}
      {isLive && (
        <>
          <label className="d-confirm__hint" htmlFor={inputId}>
            Type <code className="d-code">{shortId}</code> to confirm
          </label>
          <TextInput
            id={inputId}
            mono
            placeholder={shortId}
            value={confirmText}
            onChange={(e) => setConfirmText(e.target.value)}
            data-testid="cleanup-confirm-input"
            data-autofocus=""
            autoComplete="off"
            spellCheck={false}
          />
        </>
      )}
    </ConfirmDialog>
  );
}
