interface Props {
  onConfirm: () => void;
  onCancel: () => void;
}

export default function ForgetRunModal({ onConfirm, onCancel }: Props) {
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
      <div className="w-[400px] rounded-lg border border-line bg-bg-2 p-4 shadow-lg">
        <h3 className="font-medium text-fg" style={{ fontSize: "13px" }}>
          Forget Run Permanently
        </h3>
        <p className="mt-2 text-fg-3" style={{ fontSize: "12px" }}>
          This will permanently delete the event log for this run. The run will
          no longer appear anywhere and the manager will lose any post-mortem
          context for it.
        </p>
        <p className="mt-2 text-fg-3" style={{ fontSize: "12px", fontWeight: 500 }}>
          This cannot be undone.
        </p>
        <div className="mt-4 flex justify-end gap-2">
          <button
            onClick={onCancel}
            className="cursor-pointer rounded-md border border-line-strong bg-bg-3 px-3 py-1.5 text-fg-2 transition-colors hover:bg-bg-4"
            style={{ fontSize: "11.5px" }}
          >
            Cancel
          </button>
          <button
            onClick={onConfirm}
            className="cursor-pointer rounded-md bg-st-failed px-3 py-1.5 text-white transition-colors hover:bg-st-failed/80"
            style={{ fontSize: "11.5px" }}
          >
            Forget
          </button>
        </div>
      </div>
    </div>
  );
}
