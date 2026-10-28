import { useEffect, useState } from "react";
import { CircleX, Loader, Sparkles } from "lucide-react";
import { EmptyState } from "../../shared/ui/kit";
import "../../pipeline/components/inspect.css";
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
    <div className="pi-session" data-testid="assistant-tab">
      <div className="pi-session__bar">
        <Sparkles size={14} strokeWidth={1.5} aria-hidden className="ei-ic" />
        <span>Assistant</span>
        {session && (
          <span className="d-mono ei-dim ei-trunc" title={session}>
            {session}
          </span>
        )}
      </div>
      {error ? (
        <div className="pi-center" data-testid="assistant-error">
          <EmptyState icon={CircleX}>
            <span role="alert" title={error}>
              Failed to start the assistant: {error}
            </span>
          </EmptyState>
        </div>
      ) : session ? (
        <TmuxTerminal session={session} expanded status="running" />
      ) : (
        <div className="pi-center" data-testid="assistant-loading">
          <EmptyState icon={Loader} spin>
            Starting the assistant
          </EmptyState>
        </div>
      )}
    </div>
  );
}
