import { useEffect } from "react";

interface Props {
  open: boolean;
  pipelineName: string;
  onReload: () => void;
  onKeep: () => void;
}

export default function PipelineChangedModal({
  open,
  pipelineName,
  onReload,
  onKeep,
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
      className="fixed inset-0 z-50 flex items-center justify-center bg-scrim"
      data-testid="pipeline-changed-modal-backdrop"
      onClick={onKeep}
    >
      <div
        className="w-[440px] rounded-lg border border-line bg-surface-1 p-4 shadow-lg text-sm"
        onClick={(e) => e.stopPropagation()}
      >
        <h3 className="font-medium text-text text-md">
          Pipeline template updated
        </h3>
        <p className="mt-2 text-text-2">
          The library version of{" "}
          <code className="rounded bg-surface-2 px-1 py-0.5 font-mono text-text">
            {pipelineName}
          </code>{" "}
          has changed since this run was created. Reload changes into this run?
        </p>
        <p className="mt-2 text-text-3 text-xs">
          Keeping the run version leaves it diverged from the library — the
          pipeline star will show that state.
        </p>
        <div className="mt-4 flex justify-end gap-2">
          <button
            onClick={onKeep}
            className="cursor-pointer rounded-md border border-line-strong bg-surface-2 px-3 py-1.5 text-text-2 transition-colors hover:bg-hover text-xs"
            data-testid="pipeline-changed-keep"
          >
            Keep run version
          </button>
          <button
            onClick={onReload}
            className="cursor-pointer rounded-md bg-primary px-3 py-1.5 font-medium text-primary-ink transition-colors hover:bg-primary-hi text-xs"
            data-testid="pipeline-changed-reload"
          >
            Reload changes
          </button>
        </div>
      </div>
    </div>
  );
}
