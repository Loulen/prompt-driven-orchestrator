import { Play, RotateCcw } from "lucide-react";
import type { NodeStatus } from "../../../shared/generated/NodeStatus";
import { RETRY_BUTTON_CLASS, RETRY_BUTTON_STYLE } from "./labels";

export default function RetryPlayButton({
  status,
  onClick,
}: {
  status: NodeStatus;
  onClick: () => void;
}) {
  if (status === "running") {
    return (
      <button
        data-testid="retry-btn"
        onClick={onClick}
        className={RETRY_BUTTON_CLASS}
        style={RETRY_BUTTON_STYLE}
      >
        <RotateCcw size={10} />
        Retry
      </button>
    );
  }

  if (status === "completed") {
    return (
      <button
        data-testid="play-retry-btn"
        onClick={onClick}
        className={RETRY_BUTTON_CLASS}
        style={RETRY_BUTTON_STYLE}
      >
        <RotateCcw size={10} />
        Retry
      </button>
    );
  }

  // #598 / ADR-0049: `interrupted` joins the "Play" set. The node-level retry
  // re-drives the interrupted work — the daemon embeds the run reopen atomically
  // (`node_retry` → `embed_reopen_for_targeted_command`), so the click that used
  // to hit "resume the run first" now just works. Without this the panel offered
  // no way out of an interrupted node (issue: an interactive node with a dead
  // session had no resume affordance).
  if (
    status === "failed" ||
    status === "stopped" ||
    status === "stale" ||
    status === "interrupted"
  ) {
    return (
      <button
        data-testid="play-retry-btn"
        onClick={onClick}
        className={RETRY_BUTTON_CLASS}
        style={RETRY_BUTTON_STYLE}
      >
        <Play size={10} />
        Play
      </button>
    );
  }

  return null;
}
