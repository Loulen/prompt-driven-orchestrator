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
      <Check size={11} className="shrink-0 text-fg-4" aria-label="up to date" />
    );
  }
  if (gap.kind === "unknown") {
    return (
      <span
        className="shrink-0 font-mono text-fg-4"
        style={{ fontSize: "10px" }}
        title="Gap unknown — the last fetch failed"
      >
        ?
      </span>
    );
  }
  const ahead = gap.kind === "ahead";
  return (
    <span
      className={`shrink-0 rounded px-1 py-0.5 font-mono font-medium ${
        ahead ? "bg-st-running-bg text-st-running" : "bg-st-await-bg text-st-await"
      }`}
      style={{ fontSize: "9.5px" }}
      data-testid="branch-gap-chip"
    >
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
  let border = "border-line-strong";
  let icon = <RefreshCw size={12} className="text-fg-3" />;
  let title = "Source branch freshness";

  switch (state.kind) {
    case "fetching":
      icon = <RefreshCw size={12} className="animate-spin text-fg-4" />;
      title = "Fetching all remotes…";
      break;
    case "unknown":
      border = "border-dashed border-fg-4";
      icon = <CloudOff size={12} className="text-fg-3" />;
      title = "Gap unknown — the last fetch failed";
      break;
    case "remote":
      border = "border-acc-border";
      icon = <Cloud size={12} className="text-acc" />;
      title = "Remote ref — as of the last fetch";
      break;
    case "no-upstream":
      icon = <Laptop size={12} className="text-fg-3" />;
      title = "No upstream branch";
      break;
    case "level":
      border = "border-acc-border";
      icon = <Check size={12} className="text-acc" />;
      title = `Up to date with ${state.upstream}`;
      break;
    case "gap":
      border = "border-st-await";
      icon = <RefreshCw size={12} className="text-st-await" />;
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
    border = "border-acc-border";
    icon = <RefreshCw size={12} className="animate-spin text-acc" />;
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
        className={`flex items-center gap-1 rounded-md border bg-bg-3 px-2 py-1.5 transition-colors hover:bg-bg-4 ${border}`}
        data-testid={`${testIdPrefix}-sync`}
        data-sync-state={running ? "fast-forwarding" : state.kind}
      >
        {icon}
        {/* The ONLY écart on the form. Icon-only whenever there is none, so the
            button stays quiet until it has something to say. */}
        {gap && !running && (
          <span
            className="font-mono font-medium text-st-await"
            style={{ fontSize: "10px" }}
            data-testid={`${testIdPrefix}-sync-gap`}
          >
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
