import type { ReactNode } from "react";
import { Maximize2, Minimize2, ExternalLink, Copy, Eye, Hand, FileUp } from "lucide-react";
import { Tooltip } from "../../shared/ui/tooltip";
import {
  TAKE_CONTROL_LABEL,
  TAKEN_OVER_NOTICE,
  watchedLabel,
} from "../model/terminalRole";

interface Props {
  session: string;
  dotClass: string;
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
export const COLLAPSE_LABEL = "Collapse terminal · Esc · click outside";
export const EXPAND_LABEL = "Expand terminal";

/** The terminal's toolbar: its session, its state, and what a human can do. */
export default function TerminalToolbar({
  session,
  dotClass,
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
  return (
    <div
      className="flex items-center gap-1.5 border-b border-line px-3 py-1.5 text-fg-3"
      style={{ fontSize: "11px" }}
      data-testid="term-toolbar"
    >
      {identity ? (
        <>
          {identity}
          <span className="mx-1 h-3 w-px bg-line-strong" aria-hidden />
        </>
      ) : (
        <span className={`h-1.5 w-1.5 rounded-full ${dotClass}`} />
      )}
      <span className="font-mono text-fg-4" style={{ fontSize: "10px" }}>
        {session}
      </span>
      <span
        className={`rounded border px-1 py-px font-mono ${
          connected
            ? "border-st-done/30 text-st-done"
            : "border-line-strong text-fg-4"
        }`}
        style={{ fontSize: "9px" }}
      >
        {statusLabel}
      </span>
      <span className="flex-1" />
      {/* #870: brief, non-blocking — the former pilot learns why its keys
          stopped reaching the pane. The hand stays next to it. */}
      {spectating && takenOver && (
        <span
          className="font-mono text-st-await"
          style={{ fontSize: "10px" }}
          data-testid="term-taken-over"
          role="status"
        >
          {TAKEN_OVER_NOTICE}
        </span>
      )}
      {/* #870: a spectator takes control in one click. */}
      {spectating && (
        <Tooltip content={TAKE_CONTROL_LABEL}>
          <button
            onClick={onTakeControl}
            aria-label={TAKE_CONTROL_LABEL}
            className="flex h-5 w-5 cursor-pointer items-center justify-center rounded text-fg-3 transition-colors hover:bg-bg-4 hover:text-fg"
            data-testid="term-take-control"
          >
            <Hand size={12} />
          </button>
        </Tooltip>
      )}
      {/* #867: the pilot knows it is watched. Nothing when solo. */}
      {watchers > 0 && (
        <Tooltip content={watchedLabel(watchers)}>
          <span
            className="flex h-5 items-center gap-0.5 px-1 font-mono text-fg-4"
            style={{ fontSize: "10px" }}
            data-testid="term-watchers"
            aria-label={watchedLabel(watchers)}
            role="status"
          >
            <Eye size={12} />
            {watchers}
          </span>
        </Tooltip>
      )}
      {actions && (
        <>
          {actions}
          <span className="mx-1 h-3 w-px bg-line-strong" aria-hidden />
        </>
      )}
      {onImport && (
        <Tooltip content={importDisabledReason ?? "Import files into this node"}>
          <button
            onClick={() => {
              if (!importDisabledReason) onImport();
            }}
            aria-disabled={importDisabledReason ? true : undefined}
            aria-label="Import files"
            className={`flex h-5 w-5 items-center justify-center rounded transition-colors ${
              importDisabledReason
                ? "cursor-default text-fg-5"
                : "cursor-pointer text-fg-3 hover:bg-bg-4 hover:text-fg"
            }`}
            data-testid="term-import"
          >
            <FileUp size={12} />
          </button>
        </Tooltip>
      )}
      {/* #772: copy the browser-side selection; the tooltip doubles as the
          discoverable hint for the keyboard shortcuts. */}
      <Tooltip
        content={
          copyFeedback === "copied"
            ? "Copied"
            : copyFeedback === "failed"
              ? "Copy failed — select the text and press Ctrl+Shift+C"
              : "Copy selection · drag to select, Ctrl+Shift+C / Ctrl+C copies, Ctrl+Shift+V / Ctrl+V pastes"
        }
      >
        <button
          onClick={onCopy}
          disabled={!hasSelection}
          aria-label="Copy selection"
          className={`flex h-5 w-5 items-center justify-center rounded transition-colors ${
            hasSelection
              ? "cursor-pointer text-fg-3 hover:bg-bg-4 hover:text-fg"
              : "cursor-default text-fg-5"
          }`}
          data-testid="term-copy"
          data-feedback={copyFeedback ?? undefined}
        >
          <Copy size={12} />
        </button>
      </Tooltip>
      {onExpand && (
        <Tooltip content={expanded ? COLLAPSE_LABEL : EXPAND_LABEL}>
          <button
            onClick={onExpand}
            className="flex h-5 w-5 cursor-pointer items-center justify-center rounded text-fg-3 transition-colors hover:bg-bg-4 hover:text-fg"
            data-testid="term-expand"
          >
            {expanded ? (
              <Minimize2 size={12} />
            ) : (
              <Maximize2 size={12} />
            )}
          </button>
        </Tooltip>
      )}
      {/* No session to attach to once the pane is frozen — offering the button
          would be an action that can only fail. */}
      {onDetach && (
        <Tooltip content="Detach to OS terminal">
          <button
            onClick={onDetach}
            className="flex h-5 w-5 cursor-pointer items-center justify-center rounded text-fg-3 transition-colors hover:bg-bg-4 hover:text-fg"
            data-testid="term-detach"
          >
            <ExternalLink size={12} />
          </button>
        </Tooltip>
      )}
    </div>
  );
}
