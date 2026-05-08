interface Props {
  onConfirm: () => void;
  onCancel: () => void;
}

export default function CleanupConfirmModal({ onConfirm, onCancel }: Props) {
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
      <div className="w-[360px] rounded-lg border border-line bg-bg-2 p-4 shadow-lg">
        <h3 className="font-medium text-fg" style={{ fontSize: "13px" }}>
          Cleanup Run
        </h3>
        <p className="mt-2 text-fg-3" style={{ fontSize: "12px" }}>
          This will remove worktrees and artifacts. Event history is kept. Proceed?
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
            Cleanup
          </button>
        </div>
      </div>
    </div>
  );
}
