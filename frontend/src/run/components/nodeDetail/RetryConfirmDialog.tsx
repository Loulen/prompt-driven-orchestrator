/** The « Retry this node? » confirmation when a retry resets downstream nodes. */
export default function RetryConfirmDialog({
  retryConfirm,
  confirmRetry,
  cancelRetry,
}: {
  retryConfirm: { affectedCount: number };
  confirmRetry: () => void;
  cancelRetry: () => void;
}) {
  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-scrim"
      data-testid="retry-confirm-backdrop"
      onClick={cancelRetry}
    >
      <div
        className="w-[360px] rounded-lg border border-line bg-surface-1 p-4 shadow-lg"
        style={{ fontSize: "12px" }}
        onClick={(e) => e.stopPropagation()}
      >
        <h3 className="font-medium text-text" style={{ fontSize: "13px" }}>
          Retry this node?
        </h3>
        <p className="mt-2 text-text-3" style={{ fontSize: "11.5px" }}>
          This will reset {retryConfirm.affectedCount} downstream{" "}
          {retryConfirm.affectedCount === 1 ? "node" : "nodes"} with
          artifacts. Continue?
        </p>
        <div className="mt-4 flex justify-end gap-2">
          <button
            data-testid="retry-confirm-cancel"
            onClick={cancelRetry}
            className="rounded-md border border-line-strong bg-surface-2 px-3 py-1.5 text-text-2 transition-colors hover:bg-hover"
            style={{ fontSize: "11.5px" }}
          >
            Cancel
          </button>
          <button
            data-testid="retry-confirm-ok"
            onClick={confirmRetry}
            className="rounded-md bg-hover px-3 py-1.5 text-primary-ink transition-colors hover:bg-hover/80"
            style={{ fontSize: "11.5px" }}
          >
            Retry
          </button>
        </div>
      </div>
    </div>
  );
}
