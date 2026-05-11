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
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60"
      data-testid="pipeline-changed-modal-backdrop"
      onClick={onKeep}
    >
      <div
        className="w-[440px] rounded-lg border border-line bg-bg-2 p-4 shadow-lg"
        style={{ fontSize: "12px" }}
        onClick={(e) => e.stopPropagation()}
      >
        <h3 className="font-medium text-fg" style={{ fontSize: "13px" }}>
          Pipeline template updated
        </h3>
        <p className="mt-2 text-fg-2">
          The library version of{" "}
          <code className="rounded bg-bg-4 px-1 py-0.5 font-mono text-fg">
            {pipelineName}
          </code>{" "}
          has changed since this run was created. Reload changes into this run?
        </p>
        <p className="mt-2 text-fg-3" style={{ fontSize: "11.5px" }}>
          Keeping the run version leaves it diverged from the library — the
          pipeline star will show that state.
        </p>
        <div className="mt-4 flex justify-end gap-2">
          <button
            onClick={onKeep}
            className="cursor-pointer rounded-md border border-line-strong bg-bg-3 px-3 py-1.5 text-fg-2 transition-colors hover:bg-bg-4"
            style={{ fontSize: "11.5px" }}
            data-testid="pipeline-changed-keep"
          >
            Keep run version
          </button>
          <button
            onClick={onReload}
            className="cursor-pointer rounded-md bg-acc px-3 py-1.5 font-medium text-[#04140d] transition-colors hover:bg-acc-dim"
            style={{ fontSize: "11.5px" }}
            data-testid="pipeline-changed-reload"
          >
            Reload changes
          </button>
        </div>
      </div>
    </div>
  );
}
