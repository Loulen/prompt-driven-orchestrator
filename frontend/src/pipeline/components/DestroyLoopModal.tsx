import { useEffect } from "react";

interface Props {
  open: boolean;
  /** Ids of the bounded regions this edge deletion would destroy (#150). */
  loopIds: string[];
  onClose: () => void;
  onConfirm: () => void;
}

/**
 * Confirmation popup shown when deleting an edge would remove a bounded region's
 * **last** cycle (ADR-0011 / #150). Deleting that edge destroys the loop: its
 * `loops:` entry, bound, and iteration state go with it. On confirm the caller
 * deletes the edge (the store drops the destroyed `loops:` entries); on cancel
 * nothing changes. Deleting a non-last cycle edge never reaches this dialog.
 */
export default function DestroyLoopModal({ open, loopIds, onClose, onConfirm }: Props) {
  useEffect(() => {
    if (!open) return;
    function handleKey(e: KeyboardEvent) {
      if (e.key === "Escape") onClose();
      if (e.key === "Enter") onConfirm();
    }
    document.addEventListener("keydown", handleKey);
    return () => document.removeEventListener("keydown", handleKey);
  }, [open, onClose, onConfirm]);

  if (!open) return null;

  const plural = loopIds.length > 1;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-scrim"
      data-testid="destroy-loop-backdrop"
      onClick={onClose}
    >
      <div
        className="flex max-h-[calc(85vh/var(--app-zoom,1))] w-[380px] max-w-[calc(90vw/var(--app-zoom,1))] flex-col rounded-lg border border-line bg-surface-1 p-4 shadow-lg text-sm"
        onClick={(e) => e.stopPropagation()}
      >
        {/* #940: bounded to the window like New Run — a long list scrolls, the
            actions stay visible. */}
        <h3 className="shrink-0 font-medium text-text text-md">
          {plural ? "Destroy these loops?" : "Destroy this loop?"}
        </h3>
        <div className="min-h-0 overflow-y-auto" data-testid="destroy-loop-body">
        <p className="mt-2 text-text-2">
          Deleting this edge will destroy {plural ? "loops" : "loop"}{" "}
          {loopIds.map((id, i) => (
            <span key={id}>
              {i > 0 && ", "}
              <code className="rounded bg-surface-2 px-1 py-0.5 font-mono text-text">{id}</code>
            </span>
          ))}
          .
        </p>
        <p className="mt-2 text-text-3 text-xs">
          It is the {plural ? "regions'" : "region's"} last cycle: the{" "}
          <code className="font-mono">loops:</code> entry, its bound, and its
          iteration state are removed with the edge.
        </p>
        </div>
        <div className="mt-4 flex shrink-0 justify-end gap-2">
          <button
            onClick={onClose}
            data-testid="destroy-loop-cancel"
            className="rounded-md border border-line-strong bg-surface-2 px-3 py-1.5 text-text-2 transition-colors hover:bg-hover text-xs"
          >
            Cancel
          </button>
          <button
            onClick={onConfirm}
            data-testid="destroy-loop-confirm"
            className="rounded-md bg-st-failed px-3 py-1.5 text-primary-ink transition-colors hover:bg-st-failed/80 text-xs"
          >
            Delete edge &amp; destroy {plural ? "loops" : "loop"}
          </button>
        </div>
      </div>
    </div>
  );
}
