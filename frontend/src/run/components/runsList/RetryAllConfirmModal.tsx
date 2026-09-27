export default function RetryAllConfirmModal({
  onConfirm,
  onCancel,
}: {
  onConfirm: () => void;
  onCancel: () => void;
}) {
  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60"
      data-testid="retry-all-backdrop"
      onClick={onCancel}
    >
      <div
        className="w-[360px] rounded-lg border border-line bg-bg-4 p-4"
        style={{ fontSize: "12px" }}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="mb-1 font-medium text-fg">Retry all nodes?</div>
        <p className="mb-4 text-fg-3" style={{ fontSize: "11px" }}>
          This archives the current run and starts a fresh run of the same pipeline
          from the beginning. The archived run stays viewable (read-only).
        </p>
        <div className="flex justify-end gap-2">
          <button
            onClick={onCancel}
            className="rounded border border-line-strong bg-bg-3 px-3 py-1 text-fg-3 transition-colors hover:text-fg"
          >
            Cancel
          </button>
          <button
            onClick={onConfirm}
            data-testid="retry-all-confirm-button"
            className="rounded bg-acc px-3 py-1 font-medium text-bg-0 transition-colors hover:bg-acc-dim"
          >
            Retry all
          </button>
        </div>
      </div>
    </div>
  );
}
