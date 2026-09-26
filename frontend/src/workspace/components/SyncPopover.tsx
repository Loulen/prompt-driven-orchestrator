/** The sync popover: the selected branch's freshness, and what can be done about it (#802, #803). */

import { Check, ChevronsRight, GitBranch, Cloud, CloudOff, Laptop, RefreshCw, TriangleAlert } from "lucide-react";
import { fastForwardLabel, shortAge, type FastForwardState, type SyncState } from "../model/branchSelect";
import { Body, GapSentence, Title } from "./SyncCopy";
import { fetchedSentence } from "../model/syncSentences";
import { FastForwardCard } from "./FastForwardCard";

export function SyncPopover({
  state,
  ff,
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
  onFetch: () => void;
  onFastForward?: () => void;
  touchesCheckout?: boolean;
  onSwitchToUpstream: () => void;
  upstream: string | null;
  branchName: string;
  purpose: "run" | "trigger";
  testIdPrefix: string;
}) {
  // #803: a fast-forward in flight defers every button — including "Fetch again",
  // whose refreshed list would land on top of a move still being decided.
  const frozen = ff.kind === "running";

  const fetchAgain = (
    <button
      type="button"
      onClick={onFetch}
      disabled={state.kind === "fetching" || frozen}
      className="flex items-center gap-1.5 rounded border border-line-strong bg-bg-3 px-2 py-1 font-medium text-fg-2 transition-colors hover:bg-bg-5 disabled:opacity-40"
      style={{ fontSize: "10.5px" }}
      data-testid={`${testIdPrefix}-sync-fetch`}
    >
      <RefreshCw size={11} className={state.kind === "fetching" ? "animate-spin" : undefined} />
      Fetch again
    </button>
  );

  const launchFromUpstream = upstream ? (
    <button
      type="button"
      onClick={onSwitchToUpstream}
      disabled={frozen}
      className="flex items-center gap-1.5 rounded border border-acc-border bg-acc-bg px-2 py-1 font-medium text-acc transition-colors hover:bg-acc/20 disabled:opacity-40"
      style={{ fontSize: "10.5px" }}
      data-testid={`${testIdPrefix}-sync-switch`}
    >
      <Cloud size={11} />
      Launch from {upstream}
    </button>
  ) : null;

  /** The same shortcut, stepped DOWN to neutral: it is the fallback, not the offer. */
  const launchFromUpstreamSecondary = upstream ? (
    <button
      type="button"
      onClick={onSwitchToUpstream}
      disabled={frozen}
      className="flex items-center gap-1.5 rounded border border-line-strong bg-bg-3 px-2 py-1 font-medium text-fg-2 transition-colors hover:bg-bg-5 disabled:opacity-40"
      style={{ fontSize: "10.5px" }}
      data-testid={`${testIdPrefix}-sync-switch`}
    >
      <Cloud size={11} />
      Launch from {upstream}
    </button>
  ) : null;

  const fastForwardButton = onFastForward ? (
    <button
      type="button"
      onClick={onFastForward}
      disabled={frozen}
      className="flex items-center gap-1.5 rounded border border-acc-border bg-acc-bg px-2 py-1 font-medium text-acc transition-colors hover:bg-acc/20 disabled:opacity-40"
      style={{ fontSize: "10.5px" }}
      data-testid={`${testIdPrefix}-sync-ff`}
    >
      <ChevronsRight size={11} />
      {frozen ? "Fast-forwarding…" : fastForwardLabel(branchName)}
    </button>
  ) : null;

  return (
    <div
      className="absolute right-0 top-full z-50 mt-1 w-[300px] rounded-md border border-line-strong bg-bg-4 p-3 shadow-xl"
      data-testid={`${testIdPrefix}-sync-popover`}
      data-sync-state={state.kind}
      data-ff-state={ff.kind}
    >
      {/* ── The fast-forward leg (#803), when the last click produced something.
          It replaces the freshness card rather than stacking under it: the person
          clicked to change the answer, so the answer is what the card must be. ── */}
      {ff.kind !== "idle" ? (
        <FastForwardCard
          ff={ff}
          branchName={branchName}
          upstream={upstream}
          fetchAgain={fetchAgain}
          fastForwardButton={fastForwardButton}
          launchFromUpstream={launchFromUpstream}
          launchFromUpstreamSecondary={launchFromUpstreamSecondary}
          onRetry={onFastForward}
          testIdPrefix={testIdPrefix}
        />
      ) : (
        <>
      {state.kind === "fetching" && (
        <>
          <Title icon={<RefreshCw size={12} className="animate-spin text-fg-3" />}>
            Fetching all remotes…
          </Title>
          <div className="mt-2 flex flex-wrap gap-1.5">{fetchAgain}</div>
        </>
      )}

      {state.kind === "gap" && (
        <>
          <Title
            icon={
              state.gap.kind === "diverged" ? (
                <GitBranch size={12} className="text-st-await" />
              ) : (
                <TriangleAlert size={12} className="text-st-await" />
              )
            }
          >
            <GapSentence branchName={branchName} gap={state.gap} upstream={state.upstream} />
          </Title>
          <Body>
            {/* #803 replaces #802's "PDO never pulls into your checkout" with a
                sentence that says what the fast-forward DOES and does not do —
                the promise is only worth making where the button exists. */}
            {fastForwardButton ? (
              <>
                {fetchedSentence(state.lastFetchAt)} Fast-forward moves{" "}
                <span className="font-mono text-fg-2">{branchName}</span> onto{" "}
                <span className="font-mono text-fg-2">{state.upstream}</span> — no merge,
                no rebase, no push.{" "}
                {touchesCheckout === false
                  ? `${branchName} isn't checked out: only the ref moves, none of your files change.`
                  : "Or launch from the upstream ref and leave it alone."}
              </>
            ) : state.gap.kind === "diverged" ? (
              <>
                A fast-forward would drop your local commit
                {state.gap.ahead > 1 ? "s" : ""}, so none is offered. Launch from the
                upstream ref, or reconcile in your terminal and fetch again.
              </>
            ) : (
              <>
                {fetchedSentence(state.lastFetchAt)} PDO never pulls into your checkout:
                launch from the upstream ref to start from the fresh commits, or keep{" "}
                <span className="font-mono text-fg-2">{branchName}</span> as is.
              </>
            )}
          </Body>
          {/* Behind and lossless: the fast-forward leads — it repairs the thing the
              person already chose, which is the whole reason #803 exists. The #802
              shortcut steps down to neutral, still there, no longer the offer. */}
          <div className="mt-2 flex flex-wrap gap-1.5">
            {fastForwardButton ?? launchFromUpstream}
            {fastForwardButton ? launchFromUpstreamSecondary : null}
            {fetchAgain}
          </div>
        </>
      )}

      {state.kind === "level" && (
        <>
          <Title icon={<Check size={12} className="text-acc" />}>
            Up to date with <span className="font-mono">{state.upstream}</span>
          </Title>
          <Body>{fetchedSentence(state.lastFetchAt)}</Body>
          <div className="mt-2 flex flex-wrap gap-1.5">{fetchAgain}</div>
        </>
      )}

      {state.kind === "unknown" && (
        <>
          <Title icon={<CloudOff size={12} className="text-fg-3" />}>
            Gap unknown — fetch failed
          </Title>
          <Body>
            {state.lastFetchAt
              ? `Last successful fetch ${shortAge(state.lastFetchAt)} ago.`
              : "No successful fetch on record."}{" "}
            Launching is still possible: the run will cut from{" "}
            <span className="font-mono text-fg-2">{branchName}</span> as it is on disk.
          </Body>
          <div
            className="mt-2 max-h-16 overflow-y-auto rounded border border-line bg-bg-2 px-1.5 py-1 font-mono text-fg-3"
            style={{ fontSize: "9.5px" }}
            data-testid={`${testIdPrefix}-sync-stderr`}
          >
            {state.error.message}
          </div>
          {/* Fetch leads here: the failure is what the user wants to retry, and
              the shortcut would launch from a ref nobody could refresh. */}
          <div className="mt-2 flex flex-wrap gap-1.5">
            {fetchAgain}
            {launchFromUpstream}
          </div>
        </>
      )}

      {state.kind === "remote" && (
        <>
          <Title icon={<Cloud size={12} className="text-acc" />}>
            Remote ref — as of last fetch
          </Title>
          {/* #804: on a Trigger, "as of last fetch" understates it — the fire
              refreshes this ref itself. Said here rather than as a second icon:
              a tracking ref is the RIGHT choice, and the popover is where someone
              already comes to ask what the fire will do about freshness. */}
          <Body>
            {fetchedSentence(state.lastFetchAt)}
            {purpose === "trigger" && (
              <span data-testid={`${testIdPrefix}-sync-trigger-note`}>
                {" "}
                Fetched before the cut on each fire. A failed fetch still fires: the Run cuts
                from the last known state and records the reason.
              </span>
            )}
          </Body>
          <div className="mt-2 flex flex-wrap gap-1.5">{fetchAgain}</div>
        </>
      )}

      {state.kind === "no-upstream" && (
        <>
          <Title icon={<Laptop size={12} className="text-fg-3" />}>No upstream branch</Title>
          <Body>
            <span className="font-mono text-fg-2">{branchName || "This branch"}</span> tracks
            nothing, so there is no gap to measure. {fetchedSentence(state.lastFetchAt)}
          </Body>
          <div className="mt-2 flex flex-wrap gap-1.5">{fetchAgain}</div>
        </>
      )}
        </>
      )}

      {/* Two standing promises, in the one place every state shows: WHEN PDO
          reaches the network, and what it will never write. `flex-wrap` because
          the second is long enough to need its own line at this width — and
          truncating a promise is worse than wrapping it. */}
      <div
        className="mt-2 flex flex-wrap items-center justify-between gap-x-2 border-t border-line pt-1.5 text-fg-5"
        style={{ fontSize: "9px" }}
      >
        <span>fetch on open · on repo change · here</span>
        <span>never merges, rebases or pushes</span>
      </div>
    </div>
  );
}
