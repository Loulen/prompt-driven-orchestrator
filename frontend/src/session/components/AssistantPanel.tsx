import { useEffect, useState } from "react";
import { Bot, Loader } from "lucide-react";
import TmuxTerminal from "./TmuxTerminal";
import { openLibraryAssistant } from "../api/assistant";

/**
 * The Assistant tab body (#302 / ADR-0048, #594 / ADR-0051): an inline `claude`
 * REPL that authors pipeline templates.
 *
 * **It creates, and never reaps.** Mounting still spawns the shared session on
 * demand, but the unmount cleanup that used to `DELETE` is gone: this component
 * unmounts whenever the panel closes, and the panel closes by itself on every
 * edit-tab switch (#385) — so reaping here killed the conversation each time the
 * user glanced at another template. The reap now lives at App level, keyed on
 * "no edit view left at all" (`useLibassistLifecycle`), with the daemon's idle
 * sweep behind it.
 *
 * It takes no props for the same reason: which template is being edited is the
 * daemon's focus, not this component's business.
 */
export default function AssistantPanel() {
  const [session, setSession] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    openLibraryAssistant()
      .then((r) => {
        if (!cancelled) setSession(r.session);
      })
      .catch((e) => {
        if (!cancelled) setError(e instanceof Error ? e.message : String(e));
      });
    return () => {
      cancelled = true;
    };
  }, []);

  return (
    <div
      className="flex min-h-0 flex-1 flex-col"
      style={{ fontSize: "11.5px" }}
      data-testid="assistant-tab"
    >
      <div className="flex items-center gap-2 border-b border-line px-3 py-2">
        <Bot size={14} className="text-text-3" />
        <span className="text-text-2" style={{ fontSize: "11px" }}>
          Pipeline Assistant
        </span>
        {session && (
          <span className="font-mono text-text-3" style={{ fontSize: "10px" }}>
            {session}
          </span>
        )}
      </div>
      {error ? (
        <div
          className="flex flex-1 items-center justify-center px-4 text-center text-st-failed"
          style={{ fontSize: "11.5px" }}
          data-testid="assistant-error"
        >
          Failed to start the assistant: {error}
        </div>
      ) : session ? (
        <TmuxTerminal session={session} expanded status="running" />
      ) : (
        <div
          className="flex flex-1 items-center justify-center gap-2 text-text-3"
          style={{ fontSize: "11.5px" }}
          data-testid="assistant-loading"
        >
          <Loader size={14} className="animate-spin" />
          Starting the assistant…
        </div>
      )}
    </div>
  );
}

