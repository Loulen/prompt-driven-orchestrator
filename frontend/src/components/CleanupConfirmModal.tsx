import { useEffect, useState } from "react";

interface Props {
  runId: string;
  onConfirm: () => void;
  onCancel: () => void;
  isLive?: boolean;
}

// Spelled out so the consequences of archiving a *live* run are explicit. The
// "hasn't finished" phrasing is asserted by the unit test — keep it literal.
const LIVE_BODY =
  "This run hasn't finished. Archiving it now kills every active node session, " +
  "removes the run's worktrees and artifacts from disk, and marks the run " +
  "archived. Event history is kept, but in-flight prompts and outputs are lost.";

export default function CleanupConfirmModal({
  runId,
  onConfirm,
  onCancel,
  isLive = false,
}: Props) {
  // The 7-hex unique suffix (clean, no dash) of the run id. Run-specific so the
  // user proves they are archiving the run they think they are.
  const shortId = runId.slice(-7);
  const [confirmText, setConfirmText] = useState("");
  const canConfirm = confirmText.trim() === shortId;

  // Escape-to-cancel parity with ConfirmDeleteModal. Deliberately NO global
  // Enter listener: on the live branch Enter-to-submit is gated by the input's
  // own onKeyDown, so it can never bypass the typed confirmation.
  useEffect(() => {
    function handleKey(e: KeyboardEvent) {
      if (e.key === "Escape") onCancel();
    }
    document.addEventListener("keydown", handleKey);
    return () => document.removeEventListener("keydown", handleKey);
  }, [onCancel]);

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
      <div className="w-[360px] rounded-lg border border-line bg-bg-2 p-4 shadow-lg">
        {isLive ? (
          <>
            <h3 className="font-medium text-fg" style={{ fontSize: "13px" }}>
              Archive this run before it completes?
            </h3>
            <p className="mt-2 text-fg-3" style={{ fontSize: "12px" }}>
              {LIVE_BODY}
            </p>
            <label className="mt-3 block text-fg-3" style={{ fontSize: "12px" }}>
              Type{" "}
              <code className="rounded bg-bg-4 px-1 py-0.5 font-mono text-fg">
                {shortId}
              </code>{" "}
              to confirm
            </label>
            <input
              className="mt-1.5 w-full rounded-md border border-line-strong bg-bg-3 px-2.5 py-1.5 font-mono text-fg placeholder:text-fg-4 focus:border-acc focus:outline-none"
              style={{ fontSize: "12px" }}
              placeholder={shortId}
              value={confirmText}
              onChange={(e) => setConfirmText(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && canConfirm) onConfirm();
              }}
              data-testid="cleanup-confirm-input"
              autoFocus
            />
          </>
        ) : (
          <>
            <h3 className="font-medium text-fg" style={{ fontSize: "13px" }}>
              Cleanup Run
            </h3>
            <p className="mt-2 text-fg-3" style={{ fontSize: "12px" }}>
              This will remove worktrees and artifacts. Event history is kept.
              Proceed?
            </p>
          </>
        )}
        <div className="mt-4 flex justify-end gap-2">
          <button
            onClick={onCancel}
            className="cursor-pointer rounded-md border border-line-strong bg-bg-3 px-3 py-1.5 text-fg-2 transition-colors hover:bg-bg-4"
            style={{ fontSize: "11.5px" }}
          >
            Cancel
          </button>
          <button
            onClick={onConfirm}
            disabled={isLive && !canConfirm}
            className={
              isLive
                ? "cursor-pointer rounded-md bg-st-failed px-3 py-1.5 text-white transition-colors hover:bg-st-failed/80 disabled:cursor-not-allowed disabled:opacity-50 disabled:hover:bg-st-failed"
                : "cursor-pointer rounded-md bg-st-failed px-3 py-1.5 text-white transition-colors hover:bg-st-failed/80"
            }
            style={{ fontSize: "11.5px" }}
            data-testid="cleanup-confirm-button"
          >
            {isLive ? "Archive run" : "Cleanup"}
          </button>
        </div>
      </div>
    </div>
  );
}
