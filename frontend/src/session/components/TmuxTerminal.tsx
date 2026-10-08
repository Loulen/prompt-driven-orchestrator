import { useCallback, useEffect, useRef, type ReactNode } from "react";
import { Tooltip } from "../../shared/ui/tooltip";
import { attachSession } from "../api/sessions";
import { READ_ONLY_HINT } from "../model/terminalRole";
import { paneIndicator, TERMINAL_TYPOGRAPHY_RESET, type PaneSource } from "../model/terminalPane";
import { usePaneMode } from "../store/usePaneMode";
import { useXtermSession } from "../store/useXtermSession";
import { useFileDropTarget } from "../../skill/store/useFileDropTarget";
import { DropOverlay } from "../../skill/components/SkillFileDropZone";
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
  /** #44: the node's identity (status dot, name, iteration, status badge), at
   *  the head of the toolbar, in place of the connection dot. */
  toolbarIdentity?: ReactNode;
  /** #44: extra gestures at the right of the toolbar, before Copy. */
  toolbarActions?: ReactNode;
  /** Files given to the node: dropped on the terminal (with the files) or
   *  asked for with the Import button (empty list). Omit ⇒ no import here (the
   *  Run shell, the assistant). */
  onImportFiles?: (files: File[]) => void;
  /** Set when the node cannot receive files right now (no live session): the
   *  Import button is off and says why. */
  importDisabledReason?: string | null;
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
  toolbarIdentity,
  toolbarActions,
  onImportFiles,
  importDisabledReason,
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

  // A file dropped anywhere on the terminal opens the import, pre-filled.
  // Always claimed while an import handler exists, so a missed drop never
  // navigates the tab to the file; the modal says when the node cannot take it.
  const handleFileDrop = useCallback(
    (dt: DataTransfer) => onImportFiles?.(Array.from(dt.files)),
    [onImportFiles],
  );
  const { dragging: fileDrag, handlers: dropHandlers } = useFileDropTarget(handleFileDrop);
  const importEnabled = onImportFiles !== undefined;

  return (
    <div
      className="relative flex flex-1 flex-col overflow-hidden"
      data-testid="tmux-terminal"
      {...(importEnabled ? dropHandlers : {})}
    >
      {importEnabled && fileDrag !== null && (
        <DropOverlay
          count={fileDrag}
          title={`Drop to import ${fileDrag} file${fileDrag === 1 ? "" : "s"} into this node`}
          hint={importDisabledReason ?? "You review them before anything is written"}
        />
      )}
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
        identity={toolbarIdentity}
        actions={toolbarActions}
        onImport={onImportFiles ? () => onImportFiles([]) : undefined}
        importDisabledReason={importDisabledReason}
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
