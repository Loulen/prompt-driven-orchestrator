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
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60"
      data-testid="destroy-loop-backdrop"
      onClick={onClose}
    >
      <div
        className="w-[380px] rounded-lg border border-line bg-bg-2 p-4 shadow-lg"
        style={{ fontSize: "12px" }}
        onClick={(e) => e.stopPropagation()}
      >
        <h3 className="font-medium text-fg" style={{ fontSize: "13px" }}>
          {plural ? "Destroy these loops?" : "Destroy this loop?"}
        </h3>
        <p className="mt-2 text-fg-2">
          Deleting this edge will destroy {plural ? "loops" : "loop"}{" "}
          {loopIds.map((id, i) => (
            <span key={id}>
              {i > 0 && ", "}
              <code className="rounded bg-bg-4 px-1 py-0.5 font-mono text-fg">{id}</code>
            </span>
          ))}
          .
        </p>
        <p className="mt-2 text-fg-3" style={{ fontSize: "11.5px" }}>
          It is the {plural ? "regions'" : "region's"} last cycle: the{" "}
          <code className="font-mono">loops:</code> entry, its bound, and its
          iteration state are removed with the edge.
        </p>
        <div className="mt-4 flex justify-end gap-2">
          <button
            onClick={onClose}
            data-testid="destroy-loop-cancel"
            className="rounded-md border border-line-strong bg-bg-3 px-3 py-1.5 text-fg-2 transition-colors hover:bg-bg-4"
            style={{ fontSize: "11.5px" }}
          >
            Cancel
          </button>
          <button
            onClick={onConfirm}
            data-testid="destroy-loop-confirm"
            className="rounded-md bg-st-failed px-3 py-1.5 text-white transition-colors hover:bg-st-failed/80"
            style={{ fontSize: "11.5px" }}
          >
            Delete edge &amp; destroy {plural ? "loops" : "loop"}
          </button>
        </div>
      </div>
    </div>
  );
}
