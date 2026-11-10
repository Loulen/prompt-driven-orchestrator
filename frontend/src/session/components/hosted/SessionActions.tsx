import { useEffect, useRef, useState, type ReactNode } from "react";
import { RotateCcw, Square } from "lucide-react";
import { Button, toast } from "../../../shared/ui/kit";
import type { HostedSession } from "../../model/hostedSession";
import { useHostedSession } from "../../store/useHostedSession";
import "./hostedSession.css";

/**
 * The gestures on a Session hébergée (CORE-125): what the host brings first
 * (`children`: « Full screen »…), then Terminate while it lives,
 * Relaunch once it ended. Each is the Opération its extension's would be; a
 * refusal shows under the buttons and in a toast.
 */
export default function SessionActions({ session: id, children }: { session: string; children?: ReactNode }) {
  const { session, terminate, relaunch } = useHostedSession(id);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // A refusal is its session's: the host may put another one in its place.
  const [shown, setShown] = useState(id);
  if (shown !== id) {
    setShown(id);
    setError(null);
  }
  const current = useRef(id);
  useEffect(() => {
    current.current = id;
  }, [id]);
  const run = (title: string, gesture: () => Promise<HostedSession>) => {
    const of = id;
    setBusy(true);
    setError(null);
    gesture()
      .catch((e: unknown) => {
        const message = e instanceof Error ? e.message : String(e);
        if (current.current === of) setError(message);
        toast(message, "error", { title });
      })
      .finally(() => setBusy(false));
  };
  const ended = session?.state === "ended";
  return (
    <div className="hs-actions" data-testid="hosted-session-actions">
      {children}
      <span className="hs-actions__fill" />
      {session &&
        (ended ? (
          <Button icon={RotateCcw} disabled={busy} onClick={() => run("Relaunch", relaunch)} data-testid="hosted-session-relaunch">
            Relaunch
          </Button>
        ) : (
          <Button
            icon={Square}
            tone="danger"
            disabled={busy}
            onClick={() => run("Terminate", terminate)}
            data-testid="hosted-session-terminate"
          >
            Terminate
          </Button>
        ))}
      {error && (
        <span className="hs-error" role="alert" data-testid="hosted-session-error">
          {error}
        </span>
      )}
    </div>
  );
}
