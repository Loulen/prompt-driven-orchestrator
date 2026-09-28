import { useEffect } from "react";

interface Props {
  open: boolean;
  pipelineId: string;
  onKeep: () => void;
  onTake: () => void;
}

export default function ConflictModal({
  open,
  pipelineId,
  onKeep,
  onTake,
}: Props) {
  useEffect(() => {
    if (!open) return;
    function handleKey(e: KeyboardEvent) {
      if (e.key === "Escape") onKeep();
    }
    document.addEventListener("keydown", handleKey);
    return () => document.removeEventListener("keydown", handleKey);
  }, [open, onKeep]);

  if (!open) return null;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60"
      data-testid="conflict-modal-backdrop"
      onClick={onKeep}
    >
      <div
        className="w-[400px] rounded-lg border border-line bg-bg-2 p-4 shadow-lg"
        style={{ fontSize: "12px" }}
        onClick={(e) => e.stopPropagation()}
      >
        <h3 className="font-medium text-fg" style={{ fontSize: "13px" }}>
          External edit conflict
        </h3>
        <p className="mt-2 text-fg-2">
          The pipeline{" "}
          <code className="rounded bg-bg-4 px-1 py-0.5 font-mono text-fg">
            {pipelineId}
          </code>{" "}
          was modified externally while you have unsaved changes.
        </p>
        <p className="mt-2 text-fg-3" style={{ fontSize: "11.5px" }}>
          Choose whether to keep your canvas changes or discard them in favor of the external version.
        </p>
        <div className="mt-4 flex justify-end gap-2">
          <button
            onClick={onKeep}
            className="rounded-md border border-line-strong bg-bg-3 px-3 py-1.5 text-fg-2 transition-colors hover:bg-bg-4"
            style={{ fontSize: "11.5px" }}
          >
            Keep canvas
          </button>
          <button
            onClick={onTake}
            className="rounded-md bg-st-await px-3 py-1.5 text-black transition-colors hover:bg-st-await/80"
            style={{ fontSize: "11.5px" }}
          >
            Take external
          </button>
        </div>
      </div>
    </div>
  );
}
