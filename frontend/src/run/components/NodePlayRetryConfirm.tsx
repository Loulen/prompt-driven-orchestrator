import type { useNodePlay } from "../store/useNodePlay";
import GestureConfirmDialog from "./GestureConfirmDialog";

/** The Retry confirmation of a card's ▶ (CORE-206), while one is pending: the node panel's window. */
export default function NodePlayRetryConfirm({ nodePlay }: { nodePlay: ReturnType<typeof useNodePlay> }) {
  const pending = nodePlay.retryConfirm;
  if (!pending) return null;
  return (
    <GestureConfirmDialog
      confirmation={pending.confirmation}
      target={pending.nodeName}
      onConfirm={() => void nodePlay.confirmRetry()}
      onClose={nodePlay.cancelRetry}
    />
  );
}
