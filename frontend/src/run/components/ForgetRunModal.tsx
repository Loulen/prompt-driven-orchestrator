interface Props {
  onConfirm: () => void;
  onCancel: () => void;
}

export default function ForgetRunModal({ onConfirm, onCancel }: Props) {
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-scrim">
      <div className="w-[400px] rounded-lg border border-line bg-surface-1 p-4 shadow-lg">
        <h3 className="font-medium text-text text-md">
          Forget Run Permanently
        </h3>
        <p className="mt-2 text-text-3 text-sm">
          This will permanently delete the event log for this run. The run will
          no longer appear anywhere and the manager will lose any post-mortem
          context for it.
        </p>
        <p className="mt-2 text-text-3 text-sm" style={{ fontWeight: 500 }}>
          This cannot be undone.
        </p>
        <div className="mt-4 flex justify-end gap-2">
          <button
            onClick={onCancel}
            className="cursor-pointer rounded-md border border-line-strong bg-surface-2 px-3 py-1.5 text-text-2 transition-colors hover:bg-hover text-xs"
          >
            Cancel
          </button>
          <button
            onClick={onConfirm}
            className="cursor-pointer rounded-md bg-st-failed px-3 py-1.5 text-primary-ink transition-colors hover:bg-st-failed/80 text-xs"
          >
            Forget
          </button>
        </div>
      </div>
    </div>
  );
}
