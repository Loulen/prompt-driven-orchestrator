import { Check, ChevronsRight, CloudOff, OctagonX, RefreshCw } from "lucide-react";
import type { FastForwardRefusal } from "../../shared/generated/FastForwardRefusal";
import { isRetryableRefusal, refusalHeadline, showsRefusalMessage, type FastForwardState } from "../model/branchSelect";
import { Title, Body } from "./SyncCopy";
import { shortPath } from "../model/syncSentences";

/**
 * What the fast-forward click produced (#803) — in flight, done, or refused.
 *
 * The refusals this card normally shows are the two that could NOT be known before
 * the click (a dirty tree, a branch held by another worktree), but it renders EVERY
 * reason the daemon can name: the popover's numbers are only as fresh as the last
 * list, so a branch can diverge between the render and the click and come back with
 * a reason the button's own precondition said was impossible. Each is named with the
 * daemon's own reason code — the code is what a person searches for and what a bug
 * report should carry — and all of them lead with the shortcut, the exit that works
 * regardless of the state of any checkout.
 */
export function FastForwardCard({
  ff,
  branchName,
  upstream,
  fetchAgain,
  fastForwardButton,
  launchFromUpstream,
  launchFromUpstreamSecondary,
  onRetry,
  testIdPrefix,
}: {
  ff: FastForwardState;
  branchName: string;
  upstream: string | null;
  fetchAgain: React.ReactNode;
  fastForwardButton: React.ReactNode;
  launchFromUpstream: React.ReactNode;
  launchFromUpstreamSecondary: React.ReactNode;
  onRetry?: () => void;
  testIdPrefix: string;
}) {
  if (ff.kind === "running") {
    return (
      <>
        <Title icon={<RefreshCw size={12} className="animate-spin text-acc" />}>
          Fast-forwarding <span className="font-mono">{branchName}</span>…
        </Title>
        <Body>Checking the checkout is clean, then moving the ref.</Body>
        {/* The buttons stay in place, deferred rather than removed: a layout that
            reshuffles under the cursor for one second is its own small betrayal.
            Closing now is safe and nothing here says otherwise — the POST is
            already out, and it finishes server-side either way. */}
        <div className="mt-2 flex flex-wrap gap-1.5">
          {fastForwardButton}
          {launchFromUpstreamSecondary}
        </div>
      </>
    );
  }

  if (ff.kind === "done") {
    const { result } = ff;
    return (
      <>
        <Title icon={<Check size={12} className="text-acc" />}>
          <span className="font-mono">{result.branch}</span> fast-forwarded to{" "}
          <span className="font-mono">{result.upstream}</span>
        </Title>
        <Body>
          {result.commits} commit{result.commits > 1 ? "s" : ""}.{" "}
          {result.moved_checkout
            ? `Your checkout at ${shortPath(result.moved_checkout)} now sits on the new tip.`
            : "No file changed: the branch was checked out nowhere, so only the ref moved."}
        </Body>
        <div
          className="mt-2 rounded border border-line bg-bg-2 px-1.5 py-1 font-mono text-fg-3"
          style={{ fontSize: "9.5px" }}
          data-testid={`${testIdPrefix}-sync-ff-moved`}
        >
          {result.from} → {result.to}
          {result.subject ? ` · ${result.subject}` : ""}
        </div>
        <div className="mt-2 flex flex-wrap gap-1.5">{fetchAgain}</div>
      </>
    );
  }

  if (ff.kind === "error") {
    return (
      <>
        <Title icon={<CloudOff size={12} className="text-fg-3" />}>
          The fast-forward could not be sent
        </Title>
        <Body>
          Nothing moved: <span className="font-mono text-fg-2">{branchName}</span> is
          exactly as it was. Launching is unaffected.
        </Body>
        <div
          className="mt-2 max-h-16 overflow-y-auto rounded border border-line bg-bg-2 px-1.5 py-1 font-mono text-fg-3"
          style={{ fontSize: "9.5px" }}
          data-testid={`${testIdPrefix}-sync-ff-error`}
        >
          {ff.message}
        </div>
        <div className="mt-2 flex flex-wrap gap-1.5">
          {launchFromUpstream}
          {fetchAgain}
        </div>
      </>
    );
  }

  if (ff.kind !== "refused") return null;
  const { refusal } = ff;
  const dirty = refusal.reason === "dirty_tree";
  return (
    <>
      <Title icon={<OctagonX size={12} className="text-st-failed" />}>
        <span className="flex flex-wrap items-baseline gap-1.5">
          <span>Can&apos;t fast-forward — {refusalHeadline(refusal.reason)}</span>
          <span
            className="rounded bg-bg-2 px-1 py-0.5 font-mono font-normal text-fg-4"
            style={{ fontSize: "9px" }}
            data-testid={`${testIdPrefix}-sync-ff-reason`}
          >
            {refusal.reason}
          </span>
        </span>
      </Title>
      <Body>
        <RefusalSentence refusal={refusal} branchName={branchName} upstream={upstream} />
      </Body>
      {/* The daemon's own sentence, verbatim, wherever the listing above is not the
          better detail. It carries what this card cannot derive — how many commits
          diverged — and it is the only thing we have for a reason this build does
          not know about. Same block as the error card: a quoted answer, not ours. */}
      {showsRefusalMessage(refusal) && (
        <div
          className="mt-2 max-h-16 overflow-y-auto rounded border border-line bg-bg-2 px-1.5 py-1 font-mono text-fg-3"
          style={{ fontSize: "9.5px" }}
          data-testid={`${testIdPrefix}-sync-ff-message`}
        >
          {refusal.message}
        </div>
      )}
      {dirty && (refusal.dirty_files?.length ?? 0) > 0 && (
        <div
          className="mt-2 max-h-16 overflow-y-auto rounded border border-line bg-bg-2 px-1.5 py-1 font-mono text-fg-3"
          style={{ fontSize: "9.5px" }}
          data-testid={`${testIdPrefix}-sync-ff-dirty`}
        >
          {refusal.dirty_files?.map((line: string) => (
            <div key={line} className="truncate">
              {line}
            </div>
          ))}
        </div>
      )}
      {/* The shortcut leads: it is the exit that works no matter what any checkout
          is doing. "Try again" only where the person can act on the cause — retrying
          a branch someone else holds is an invitation to a loop. */}
      <div className="mt-2 flex flex-wrap gap-1.5">
        {launchFromUpstream}
        {onRetry && isRetryableRefusal(refusal) && (
          <button
            type="button"
            onClick={onRetry}
            className="flex items-center gap-1.5 rounded border border-line-strong bg-bg-3 px-2 py-1 font-medium text-fg-2 transition-colors hover:bg-bg-5"
            style={{ fontSize: "10.5px" }}
            data-testid={`${testIdPrefix}-sync-ff-retry`}
          >
            <ChevronsRight size={11} />
            Try again
          </button>
        )}
        {fetchAgain}
      </div>
    </>
  );
}

/**
 * What the refusal MEANS, in the reader's terms — one sentence per reason (#803).
 *
 * Every arm names a cause that is actually true of the state the daemon refused on,
 * and points at the exit for that cause: commit or stash for a dirty tree, another
 * checkout to switch for a held branch, a reconcile in the terminal for a divergence.
 * The default arm deliberately explains nothing and leaves the daemon's message
 * (rendered by the caller) to speak — inventing a cause is how the first version of
 * this card told people to look for a worktree that did not exist.
 */
function RefusalSentence({
  refusal,
  branchName,
  upstream,
}: {
  refusal: FastForwardRefusal;
  branchName: string;
  upstream: string | null;
}) {
  const name = <span className="font-mono text-fg-2">{branchName}</span>;
  const up = upstream ? (
    <span className="font-mono text-fg-2">{upstream}</span>
  ) : (
    "its upstream"
  );
  const checkout = (
    <span className="font-mono text-fg-2">
      {refusal.checkout ? shortPath(refusal.checkout) : "another checkout"}
    </span>
  );

  switch (refusal.reason) {
    case "dirty_tree": {
      const count = refusal.dirty_count ?? refusal.dirty_files?.length ?? 0;
      return (
        <>
          {name} is checked out at {checkout} and {count} tracked file
          {count > 1 ? "s are" : " is"} modified. PDO won&apos;t touch them. Commit or
          stash, then try again
          {upstream ? <> — or launch from {up}.</> : "."}
        </>
      );
    }
    case "checked_out_elsewhere":
      return (
        <>
          {name} is checked out at {checkout}. PDO only fast-forwards a branch in the
          repository you launch from.
          {upstream ? <> Launch from {up} instead.</> : ""}
        </>
      );
    case "diverged":
      return (
        <>
          {name} has commits {up} does not, so a fast-forward would drop them — and PDO
          never merges or rebases. Reconcile in your terminal, or launch from {up} and
          leave {name} exactly as it is.
        </>
      );
    case "no_upstream":
      return (
        <>
          {name} tracks nothing any more, so there is no branch to fast-forward onto.
          Nothing moved.
        </>
      );
    case "up_to_date":
      return (
        <>
          {name} already has everything {up} has — the list this popover was drawn from
          was a beat old. Nothing moved.
        </>
      );
    default:
      return (
        <>
          Nothing moved: {name} is exactly as it was. The daemon&apos;s reason is below.
        </>
      );
  }
}
