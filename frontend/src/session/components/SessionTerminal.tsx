import { useEffect, useState, type ReactNode } from "react";
import { useDaemonSocket } from "../../run/store/useDaemonSocket";
import TmuxTerminal from "./TmuxTerminal";
import { useTerminalZoom } from "../store/useTerminalZoom";
import { terminalFontFactor } from "../model/terminalZoom";
import "../../run/components/inspector.css";

export interface SessionTerminalProps {
  /** The session's id: its tmux session's name (a Session hébergée's `id`,
   *  `pdo-hosted-…`, or any other session the daemon holds). */
  session: string;
  /** A zoom the host imposes, as a factor (`1`: none), multiplied with the
   *  session's Zoom du terminal. */
  zoom?: number;
  /** At the head of the toolbar, in place of the connection dot. */
  toolbarIdentity?: ReactNode;
  /** At the right of the toolbar, before Copy. */
  toolbarActions?: ReactNode;
  /** Shows the « collapse » button instead of « expand ». */
  expanded?: boolean;
  /** Adds the expand button to the toolbar. */
  onExpand?: () => void;
  /** Whether a live socket is open on the session. */
  onLiveSocketChange?: (live: boolean) => void;
  /** Whether this browser only watches (another Poste pilots). */
  onSpectatingChange?: (spectating: boolean) => void;
}

/**
 * The session terminal `@pdo/extension-ui` hands to the extensions (CORE-124):
 * the core's own, for any session by its id. Poste, Pilote, Spectateur and
 * Prise de main, resizing and reconnection are the core's; its text follows
 * the session's Zoom du terminal, live, times the zoom the host imposes. A
 * Session hébergée relaunched after it ended is connected to anew.
 */
export default function SessionTerminal({ session, zoom = 1, ...rest }: SessionTerminalProps) {
  const { zoom: sessionZoom } = useTerminalZoom(session);
  const launch = useLaunchCount(session);
  return (
    <TmuxTerminal
      key={`${session}:${launch}`}
      session={session}
      fontScale={terminalFontFactor(sessionZoom, zoom)}
      {...rest}
    />
  );
}

/** How many times the Session hébergée `session` came up again after a
 *  relaunch (CORE-125): its terminal connects anew to the relaunched process
 *  instead of staying on the closed one. A relaunch publishes `starting`
 *  (recorded before its process exists), then `running`; an ended session the
 *  page opened on counts as well. */
function useLaunchCount(session: string): number {
  const { subscribe } = useDaemonSocket();
  const [count, setCount] = useState(0);
  useEffect(() => {
    let relaunching = false;
    return subscribe((msg) => {
      if (msg.type !== "hosted_session_changed" || msg.session?.id !== session) return;
      if (msg.session.state === "ended" || msg.session.state === "starting") relaunching = true;
      else if (relaunching && msg.session.state === "running") {
        relaunching = false;
        setCount((n) => n + 1);
      }
    });
  }, [session, subscribe]);
  return count;
}
