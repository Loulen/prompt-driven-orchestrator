import type { ReactNode } from "react";
import { Maximize2, Minimize2, ExternalLink, Copy, Eye, Hand, FileUp, Keyboard, Lock, type LucideIcon } from "lucide-react";
import { IconButton, Tooltip } from "../../shared/ui/kit";
import {
  TAKE_CONTROL_LABEL,
  TAKEN_OVER_NOTICE,
  watchedLabel,
} from "../model/terminalRole";

interface Props {
  session: string;
  /** The connection's dot, kept for the callers; the bar names the mode instead. */
  dotClass?: string;
  statusLabel: string;
  connected: boolean;
  /** #870: this browser watches another poste's terminal. */
  spectating: boolean;
  takenOver: boolean;
  /** #867: other postes watching this browser's terminal. */
  watchers: number;
  hasSelection: boolean;
  copyFeedback: "copied" | "failed" | null;
  expanded: boolean;
  /** Absent once the pane is frozen: there is no session left to detach. */
  onDetach?: () => void;
  onExpand?: () => void;
  onCopy: () => void;
  onTakeControl: () => void;
  /** #44: the node's identity, at the head of the toolbar, in place of the
   *  connection dot (the « attached · live » badge still says how the socket is
   *  doing). */
  identity?: ReactNode;
  /** #44: extra gestures at the right of the toolbar, before Copy, followed by
   *  a separator. */
  actions?: ReactNode;
  /** Import files from this machine into the node. Absent: no Import button. */
  onImport?: () => void;
  /** Why the node cannot receive files now: the button is off and says it. */
  importDisabledReason?: string | null;
}

/** #44: the exits of the enlarged terminal, named on its collapse button. */
export const COLLAPSE_LABEL = "Collapse terminal";
export const EXPAND_LABEL = "Expand terminal";

/** The mode the bar names: who drives the session, or why nobody does. */
function modeOf(connected: boolean, spectating: boolean, statusLabel: string): { icon: LucideIcon; role: "pilot" | "spectator" | null } {
  if (spectating) return { icon: Eye, role: "spectator" };
  if (connected && statusLabel === "attached · live") return { icon: Keyboard, role: "pilot" };
  return { icon: connected ? Keyboard : Lock, role: null };
}

/**
 * Who drives the session, as a chip with a ruby accent: the pilot reads « You
 * pilot » and how many watch; a spectator reads « Watching » and takes control
 * from the chip itself (#867, #870).
 */
function RoleChip({ role, watchers, onTakeControl }: { role: "pilot" | "spectator"; watchers: number; onTakeControl: () => void }) {
  if (role === "pilot") {
    return (
      <span className="ri-term-bar__role" data-role="pilot" data-testid="term-mode">
        <span className="ri-term-bar__role-dot" aria-hidden />
        <span>You pilot</span>
        {/* #867: the pilot knows it is watched. Nothing after the rule when solo. */}
        {watchers > 0 && (
          <>
            <span className="ri-term-bar__role-sep" aria-hidden />
            <Tooltip label={watchedLabel(watchers)}>
              <span className="ri-term-bar__role-part" data-testid="term-watchers" aria-label={watchedLabel(watchers)} role="status">
                <Eye size={11} strokeWidth={1.5} aria-hidden />
                {watchers} watching
              </span>
            </Tooltip>
          </>
        )}
      </span>
    );
  }
  return (
    <span className="ri-term-bar__role" data-role="spectator" data-testid="term-mode">
      <span className="ri-term-bar__role-part">
        <Eye size={11} strokeWidth={1.5} aria-hidden />
        Watching
      </span>
      <span className="ri-term-bar__role-sep" aria-hidden />
      {/* #870: a spectator takes control in one click. */}
      <button type="button" className="ri-term-bar__role-action" data-testid="term-take-control" onClick={onTakeControl}>
        <Hand size={11} strokeWidth={1.5} aria-hidden />
        {TAKE_CONTROL_LABEL}
      </button>
    </span>
  );
}

/**
 * The terminal's session bar, inside its always-dark screen: the mode icon,
 * the session, who drives it (the pilot / spectator chip) or why nobody does,
 * then the tools.
 */
export default function TerminalToolbar({
  session,
  statusLabel,
  connected,
  spectating,
  takenOver,
  watchers,
  hasSelection,
  copyFeedback,
  expanded,
  onDetach,
  onExpand,
  onCopy,
  onTakeControl,
  identity,
  actions,
  onImport,
  importDisabledReason,
}: Props) {
  const mode = modeOf(connected, spectating, statusLabel);
  const copyLabel =
    copyFeedback === "copied" ? "Copied" : copyFeedback === "failed" ? "Copy failed: select and press Ctrl+Shift+C" : "Copy selection";
  return (
    <div className="ri-term-bar" data-testid="term-toolbar">
      {identity ? (
        <>
          {identity}
          <span className="ri-term-bar__sep" aria-hidden />
        </>
      ) : (
        <mode.icon size={12} strokeWidth={1.5} aria-hidden />
      )}
      <span className="ri-term-bar__session" title={session}>
        {session}
      </span>
      {mode.role ? (
        <RoleChip role={mode.role} watchers={watchers} onTakeControl={onTakeControl} />
      ) : (
        <span className="ri-term-bar__tag" data-testid="term-mode">
          {statusLabel}
        </span>
      )}
      {/* #870: brief, non-blocking — the former pilot learns why its keys
          stopped reaching the pane. « Take control » stays in the chip. */}
      {spectating && takenOver && (
        <span className="ri-term-bar__notice" data-testid="term-taken-over" role="status">
          {TAKEN_OVER_NOTICE}
        </span>
      )}
      <span className="ri-term-bar__tools">
        {actions && (
          <>
            {actions}
            <span className="ri-term-bar__sep" aria-hidden />
          </>
        )}
        {onImport && (
          <IconButton
            icon={FileUp}
            size="sm"
            label={importDisabledReason ?? "Import files into this node"}
            aria-disabled={importDisabledReason ? true : undefined}
            data-testid="term-import"
            onClick={() => {
              if (!importDisabledReason) onImport();
            }}
          />
        )}
        {/* #772: copy the browser-side selection. */}
        <IconButton
          icon={Copy}
          size="sm"
          label={copyLabel}
          kbd={copyFeedback ? undefined : "Ctrl+Shift+C"}
          disabled={!hasSelection}
          data-testid="term-copy"
          data-feedback={copyFeedback ?? undefined}
          onClick={onCopy}
        />
        {onExpand && (
          <IconButton
            icon={expanded ? Minimize2 : Maximize2}
            size="sm"
            label={expanded ? COLLAPSE_LABEL : EXPAND_LABEL}
            kbd={expanded ? "Esc" : undefined}
            data-testid="term-expand"
            onClick={onExpand}
          />
        )}
        {/* No session to attach to once the pane is frozen — offering the button
            would be an action that can only fail. */}
        {onDetach && (
          <IconButton icon={ExternalLink} size="sm" label="Detach to OS terminal" data-testid="term-detach" onClick={onDetach} />
        )}
      </span>
    </div>
  );
}
