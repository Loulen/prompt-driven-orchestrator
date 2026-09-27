import { useCallback, useEffect, useRef } from "react";
import { Tooltip } from "../../shared/ui/tooltip";
import { attachSession } from "../api/sessions";
import { READ_ONLY_HINT } from "../model/terminalRole";
import { paneIndicator, TERMINAL_TYPOGRAPHY_RESET, type PaneSource } from "../model/terminalPane";
import { usePaneMode } from "../store/usePaneMode";
import { useXtermSession } from "../store/useXtermSession";
import TerminalToolbar from "./TerminalToolbar";

export type { PaneSource } from "../model/terminalPane";

interface Props {
  session: string;
  expanded?: boolean;
  onExpand?: () => void;
  status?: string;
  /** #617: where to read the post-mortem pane once the node's iteration is
   *  terminal. Omit ⇒ live attach only (the Run shell has no node identity). */
  paneSource?: PaneSource;
  /** #588: tells the parent whether a live PTY socket is open (`true`) or the
   *  pane is a frozen snapshot / disconnected (`false`), so the awaiting banner
   *  can say « reply below and press Enter » only when Enter can reach anything. */
  onLiveSocketChange?: (live: boolean) => void;
  /** #870: tells the parent whether this browser is a spectator of the live
   *  terminal, so the awaiting banner says « take control to reply » instead of
   *  « reply below ». */
  onSpectatingChange?: (spectating: boolean) => void;
}

/** A session's inline terminal (ADR-0005): the live PTY, or its frozen pane. */
export default function TmuxTerminal({
  session,
  expanded = false,
  onExpand,
  status,
  paneSource,
  onLiveSocketChange,
  onSpectatingChange,
}: Props) {
  const containerRef = useRef<HTMLDivElement>(null);
  const { mode, frozen } = usePaneMode(session, status, paneSource);
  const xterm = useXtermSession(containerRef, session, mode, frozen);
  const { connected, termRole } = xterm;

  // #588: report the live-socket state to the parent (banner hint A1 vs A4).
  useEffect(() => {
    onLiveSocketChange?.(mode === "live" && connected);
  }, [mode, connected, onLiveSocketChange]);

  const handleDetach = useCallback(async () => {
    try {
      await attachSession(session);
    } catch (e) {
      console.error("Failed to detach terminal:", e);
    }
  }, [session]);

  const { dotClass, statusLabel } = paneIndicator(mode, connected, status, frozen);
  const spectating = mode === "live" && termRole.role === "spectator";
  const watchers =
    mode === "live" && termRole.role === "pilot" ? termRole.spectators : 0;

  // #870: report the spectator state to the parent (banner hint).
  useEffect(() => {
    onSpectatingChange?.(spectating);
  }, [spectating, onSpectatingChange]);

  return (
    <div
      className="flex flex-1 flex-col overflow-hidden"
      data-testid="tmux-terminal"
    >
      <TerminalToolbar
        session={session}
        dotClass={dotClass}
        statusLabel={statusLabel}
        connected={connected}
        spectating={spectating}
        takenOver={xterm.takenOver}
        watchers={watchers}
        hasSelection={xterm.hasSelection}
        copyFeedback={xterm.copyFeedback}
        expanded={expanded}
        onDetach={mode !== "frozen" ? () => void handleDetach() : undefined}
        onExpand={onExpand}
        onCopy={xterm.copySelection}
        onTakeControl={xterm.takeControl}
      />

      {/* Terminal container. #867: the read-only hint is always mounted and
          only enabled for a spectator — unwrapping the container instead would
          remount it and lose the xterm inside. A spectator's grid may be larger
          than the area at the font floor: then the area scrolls. */}
      <Tooltip content={READ_ONLY_HINT} disabled={!spectating}>
        <div
          ref={containerRef}
          className={`min-h-0 flex-1 bg-bg-0${spectating ? " overflow-auto" : ""}`}
          style={TERMINAL_TYPOGRAPHY_RESET}
          data-testid="xterm-container"
          data-role={mode === "live" ? termRole.role : undefined}
        />
      </Tooltip>
    </div>
  );
}
