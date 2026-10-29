import { Check, Cloud, CloudOff, Laptop, RefreshCw } from "lucide-react";
import { gapLabel, type BranchGap, type FastForwardState, type SyncState } from "../model/branchSelect";
import { SyncPopover } from "./SyncPopover";

/**
 * The per-row écart. Nothing at all for a remote ref or a branch that tracks no
 * upstream — the absence IS the statement (there is no comparison to make), and
 * a `✓` there would claim an agreement that does not exist.
 */
export function GapChip({ gap }: { gap: BranchGap | null }) {
  if (!gap) return null;
  if (gap.kind === "level") {
    return (
      <Check size={12} strokeWidth={1.5} className="shrink-0 text-text-3" aria-label="up to date" />
    );
  }
  if (gap.kind === "unknown") {
    return (
      <span className="ln-gap ln-gap--unknown" title="Gap unknown — the last fetch failed">
        ?
      </span>
    );
  }
  const ahead = gap.kind === "ahead";
  return (
    <span className="d-chip d-chip--mono ln-gap" data-ahead={ahead || undefined} data-testid="branch-gap-chip">
      {gapLabel(gap)}
    </span>
  );
}

export function SyncButton({
  state,
  ff,
  open,
  onToggle,
  onFetch,
  onFastForward,
  touchesCheckout,
  onSwitchToUpstream,
  upstream,
  branchName,
  purpose,
  testIdPrefix,
}: {
  state: SyncState;
  ff: FastForwardState;
  open: boolean;
  onToggle: () => void;
  onFetch: () => void;
  /** `undefined` = no fast-forward is on offer for this selection. */
  onFastForward?: () => void;
  touchesCheckout?: boolean;
  onSwitchToUpstream: () => void;
  upstream: string | null;
  branchName: string;
  purpose: "run" | "trigger";
  testIdPrefix: string;
}) {
  const gap = state.kind === "gap" ? state.gap : null;
  // The button's tone (`data-tone`): its icon's colour, and its edge.
  let tone: "quiet" | "unknown" | "ok" | "gap" = "quiet";
  let icon = <RefreshCw size={13} strokeWidth={1.5} aria-hidden />;
  let title = "Source branch freshness";

  switch (state.kind) {
    case "fetching":
      icon = <RefreshCw size={13} strokeWidth={1.5} className="d-spin" aria-hidden />;
      title = "Fetching all remotes…";
      break;
    case "unknown":
      tone = "unknown";
      icon = <CloudOff size={13} strokeWidth={1.5} aria-hidden />;
      title = "Gap unknown — the last fetch failed";
      break;
    case "remote":
      tone = "ok";
      icon = <Cloud size={13} strokeWidth={1.5} aria-hidden />;
      title = "Remote ref — as of the last fetch";
      break;
    case "no-upstream":
      icon = <Laptop size={13} strokeWidth={1.5} aria-hidden />;
      title = "No upstream branch";
      break;
    case "level":
      tone = "ok";
      icon = <Check size={13} strokeWidth={1.5} aria-hidden />;
      title = `Up to date with ${state.upstream}`;
      break;
    case "gap":
      tone = "gap";
      icon = <RefreshCw size={13} strokeWidth={1.5} aria-hidden />;
      title = `${gapLabel(state.gap)} vs ${state.upstream}`;
      break;
  }

  // #803: while the fast-forward is out, the trigger says so rather than keep
  // showing the écart it is in the middle of erasing. There is deliberately NO
  // `done` case: the refreshed list has already turned the state to `level`, so
  // the ✓ is the same ✓ any up-to-date branch gets — one source of truth for
  // "this branch is level", not a badge we paint ourselves after a click.
  const running = ff.kind === "running";
  if (running) {
    tone = "ok";
    icon = <RefreshCw size={13} strokeWidth={1.5} className="d-spin" aria-hidden />;
    title = `Fast-forwarding ${branchName}…`;
  }

  return (
    <div className="relative shrink-0">
      <button
        type="button"
        onClick={onToggle}
        title={title}
        aria-label={title}
        aria-expanded={open}
        className="d-btn ln-sync"
        data-tone={tone}
        data-testid={`${testIdPrefix}-sync`}
        data-sync-state={running ? "fast-forwarding" : state.kind}
      >
        {icon}
        {/* The ONLY écart on the form. Icon-only whenever there is none, so the
            button stays quiet until it has something to say. */}
        {gap && !running && (
          <span className="ln-sync__gap" data-testid={`${testIdPrefix}-sync-gap`}>
            {gapLabel(gap)}
          </span>
        )}
      </button>

      {open && (
        <SyncPopover
          state={state}
          ff={ff}
          onFetch={onFetch}
          onFastForward={onFastForward}
          touchesCheckout={touchesCheckout}
          onSwitchToUpstream={onSwitchToUpstream}
          upstream={upstream}
          branchName={branchName}
          purpose={purpose}
          testIdPrefix={testIdPrefix}
        />
      )}
    </div>
  );
}
