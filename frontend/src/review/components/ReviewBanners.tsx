import { deliveryOfPair, WORKTREE_ID } from "../../run/model/runRefs";
import type { RunRefs } from "../../shared/generated/RunRefs";
import type { RunState } from "../../shared/generated/RunState";
import type { RefPair } from "../../run/model/runRefs";

interface Props {
  run: RunState | null;
  refs: RunRefs | null;
  pair: RefPair;
  setPair: (next: RefPair) => void;
  defaults: RefPair;
  isArchived: boolean;
  tipMoved: boolean;
  nodeDelivered: { nodeId: string; iter: number } | null;
  reload: () => void;
  sendDisabledReason: string | null;
  notice: string | null;
  setNotice: (notice: string | null) => void;
}

/**
 * The banners above the Review page's diff (#749, #750, #835): the Run tip
 * moved since the diff was loaded (Reload), the running Run's working tree as
 * of load, a node delivered while its live branch was on display, why sending
 * is disabled, a fallen-back pair, and the delivery under review (Whole Run
 * instead).
 */
export default function ReviewBanners({
  run,
  refs,
  pair,
  setPair,
  defaults,
  isArchived,
  tipMoved,
  nodeDelivered,
  reload,
  sendDisabledReason,
  notice,
  setNotice,
}: Props) {
  const delivery = refs ? deliveryOfPair(pair, refs) : null;
  const defaultLabel = `${defaults.from} → ${defaults.to}`;
  const showsWorktree = pair.to === WORKTREE_ID;
  return (
    <>
      {tipMoved && !isArchived && (
        <div
          className="sticky top-0 z-[15] flex items-center gap-2.5 border-b border-st-running/35 bg-st-running/14 px-3 py-1.5 text-st-running"
          style={{ fontSize: "11px" }}
          data-testid="review-tip-moved"
        >
          <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-current" />
          The Run tip moved since this diff was loaded ·
          <button
            type="button"
            onClick={reload}
            className="cursor-pointer rounded border border-st-running/50 px-2 py-px hover:bg-st-running/20"
          >
            Reload
          </button>
          <span className="text-text-3">Your scroll position is kept.</span>
        </div>
      )}
      {showsWorktree && !tipMoved && !isArchived && run?.status === "running" && (
        <div
          className="flex items-center gap-2.5 border-b border-line bg-surface-1 px-3 py-1 text-text-3"
          style={{ fontSize: "10.5px" }}
          data-testid="review-worktree-note"
        >
          <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-st-running" />
          Working tree as of load — commits and uncommitted edits of the running Run ·
          <button
            type="button"
            onClick={reload}
            className="cursor-pointer rounded border border-line-strong px-2 py-px hover:bg-hover"
          >
            Reload
          </button>
        </div>
      )}
      {nodeDelivered && refs && (
        <div
          className="sticky top-0 z-[15] flex items-center gap-2.5 border-b border-st-running/35 bg-st-running/14 px-3 py-1.5 text-st-running"
          style={{ fontSize: "11px" }}
          data-testid="review-node-delivered"
        >
          Node delivered · its live branch is gone ·
          <button
            type="button"
            onClick={() =>
              setPair({
                from: `node:${nodeDelivered.nodeId}:${nodeDelivered.iter}:before`,
                to: `node:${nodeDelivered.nodeId}:${nodeDelivered.iter}:after`,
              })
            }
            className="cursor-pointer rounded border border-st-running/50 px-2 py-px hover:bg-st-running/20"
          >
            Switch to its after ref
          </button>
        </div>
      )}

      {sendDisabledReason && !isArchived && (
        <div
          className="mx-3 mt-2.5 flex items-center gap-2 rounded-md border border-st-await/35 bg-st-await/14 px-2.5 py-1.5 text-text-2"
          style={{ fontSize: "11px" }}
          data-testid="review-branch-gone"
        >
          ⚠ <span>{sendDisabledReason}</span>
        </div>
      )}

      {notice && (
        <div
          className="mx-3 mt-2.5 flex items-center gap-2 rounded-md border border-st-await/35 bg-st-await/14 px-2.5 py-1.5 text-text-2"
          style={{ fontSize: "11px" }}
          data-testid="review-notice"
        >
          {notice}
          <button type="button" onClick={() => setNotice(null)} className="ml-auto cursor-pointer text-text-3 hover:text-text">
            ✕
          </button>
        </div>
      )}

      {delivery && !isArchived && (
        <div
          className="mx-3 mt-2.5 flex items-center gap-2 rounded-md border border-primary/30 bg-selected px-2.5 py-1.5 text-text-2"
          style={{ fontSize: "11px" }}
          data-testid="review-delivery-chip"
        >
          ◈ Reviewing the delivery of <span className="font-medium text-primary">{delivery.node_name}</span> · iter{" "}
          {delivery.iter}
          {delivery.status === "running" && (
            <span className="text-text-3">(live sub-worktree, not merged back yet)</span>
          )}
          <button
            type="button"
            onClick={() => setPair(defaults)}
            className="ml-auto cursor-pointer text-text-3 hover:text-text"
            style={{ fontSize: "10.5px" }}
            data-testid="review-whole-run"
          >
            Whole Run instead ({defaultLabel})
          </button>
        </div>
      )}
    </>
  );
}
