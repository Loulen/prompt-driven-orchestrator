import { useEffect } from "react";
import { X } from "lucide-react";
import TmuxTerminal from "./TmuxTerminal";

interface Props {
  /** tmux session to attach to (e.g. `pdo-shell-<run-id>`), from `openRunShell`. */
  session: string;
  onClose: () => void;
}

/**
 * Ad-hoc bash shell in a terminal run's pipeline worktree (#316 / ADR-0021).
 *
 * A thin frame around the existing inline `TmuxTerminal`: the daemon has already
 * created (or re-attached) the `pdo-shell-<run-id>` session, and the terminal's
 * PTY WebSocket (`WS /sessions/<session>/pty`) opens itself from the session
 * name — no attach API to call here. The shell session is *persistent*: closing
 * this modal detaches (tmux keeps it alive), so a re-open re-attaches the same
 * scrollback.
 */
export default function RunShellModal({ session, onClose }: Props) {
  // Escape-to-close parity with CleanupConfirmModal.
  useEffect(() => {
    function handleKey(e: KeyboardEvent) {
      if (e.key === "Escape") onClose();
    }
    document.addEventListener("keydown", handleKey);
    return () => document.removeEventListener("keydown", handleKey);
  }, [onClose]);

  return (
    <div
      className="fixed inset-0 z-50 grid place-items-center"
      style={{ background: "rgba(5,7,10,0.66)", backdropFilter: "blur(4px)" }}
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
      data-testid="run-shell-modal"
    >
      <div
        className="flex h-[80vh] w-[860px] max-w-[92vw] flex-col overflow-hidden rounded-lg border border-line-strong bg-bg-2"
        style={{ boxShadow: "0 30px 80px rgba(0,0,0,0.6)" }}
      >
        {/* Header */}
        <div className="flex items-center justify-between border-b border-line px-4 py-3">
          <div className="flex items-center gap-2">
            <span className="font-medium text-fg" style={{ fontSize: "13px" }}>
              Run shell
            </span>
            <span className="font-mono text-fg-4" style={{ fontSize: "10px" }}>
              {session}
            </span>
          </div>
          <button
            onClick={onClose}
            className="rounded p-1 text-fg-3 hover:bg-bg-3 hover:text-fg"
            aria-label="Close shell"
            data-testid="run-shell-close"
          >
            <X size={14} />
          </button>
        </div>

        {/* Body — the inline terminal fills the remaining height. */}
        <div className="flex min-h-0 flex-1 flex-col">
          <TmuxTerminal session={session} expanded />
        </div>
      </div>
    </div>
  );
}
