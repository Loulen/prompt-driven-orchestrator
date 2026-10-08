import { useEffect, useState } from "react";
import { TriangleAlert } from "lucide-react";
import { fetchRunPipelineOverwritePreview, type OverwritePreview } from "../../run/api/runPipeline";
import { useEditStore } from "../store/editStore";
import type { OpenPipeline } from "../store/editState";

/**
 * ADR-0085: the warning before « Overwrite default pipeline ». It says who is
 * touched (every future run, the Triggers that fire the pipeline) and whether
 * the shared pipeline changed since this run was launched. Confirmed, the
 * run's whole snapshot (graph and prompts) replaces it. No Enter binding: the
 * gesture is the dangerous one, it must be clicked.
 */
export default function OverwriteDefaultPipelineModal({
  tab,
  onClose,
}: {
  tab: OpenPipeline & { runId: string };
  onClose: () => void;
}) {
  const overwrite = useEditStore((s) => s.overwriteDefaultPipeline);
  const runId = tab.runId;
  const [preview, setPreview] = useState<OverwritePreview | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    fetchRunPipelineOverwritePreview(runId)
      .then((p) => { if (live) setPreview(p); })
      .catch((e: unknown) => { if (live) setLoadError(e instanceof Error ? e.message : String(e)); });
    return () => { live = false; };
  }, [runId]);

  useEffect(() => {
    function handleKey(e: KeyboardEvent) {
      if (e.key === "Escape" && !busy) onClose();
    }
    document.addEventListener("keydown", handleKey);
    return () => document.removeEventListener("keydown", handleKey);
  }, [onClose, busy]);

  async function confirm() {
    setBusy(true);
    setError(null);
    try {
      await overwrite(tab.id);
      onClose();
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : String(e));
      setBusy(false);
    }
  }

  const pipelineName = preview?.pipeline_id ?? tab.pipeline.name;
  const triggers = preview?.triggers ?? [];
  const canConfirm = preview != null && preview.pipeline_exists && !busy;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60"
      data-testid="overwrite-default-backdrop"
      onClick={() => { if (!busy) onClose(); }}
    >
      <div
        role="dialog"
        aria-label="Overwrite default pipeline"
        data-testid="overwrite-default-modal"
        className="flex max-h-[85vh] w-[440px] max-w-[90vw] flex-col rounded-lg border border-line bg-bg-2 p-4 shadow-lg"
        style={{ fontSize: "12px" }}
        onClick={(e) => e.stopPropagation()}
      >
        <h3 className="shrink-0 font-medium text-fg" style={{ fontSize: "13px" }}>
          Overwrite the default pipeline?
        </h3>
        <div className="mt-2 flex min-h-0 flex-col gap-2 overflow-y-auto text-fg-2" data-testid="overwrite-default-body">
          <p data-testid="overwrite-default-future-runs">
            This run's version (graph and prompts) replaces{" "}
            <code className="rounded bg-bg-4 px-1 py-0.5 font-mono text-fg">{pipelineName}</code>.{" "}
            <strong className="font-medium text-fg">All future runs will use it.</strong> Runs already
            started keep their own copy.
          </p>
          {loadError && (
            <p className="text-st-failed" data-testid="overwrite-default-load-error">
              Could not read what the overwrite would touch: {loadError}
            </p>
          )}
          {preview == null && loadError == null && (
            <p className="text-fg-4" data-testid="overwrite-default-loading">Checking triggers and changes…</p>
          )}
          {preview != null && !preview.pipeline_exists && (
            <p className="text-st-failed" data-testid="overwrite-default-missing">
              The default pipeline no longer exists (renamed or deleted): there is nothing to overwrite.
            </p>
          )}
          {preview != null && preview.pipeline_exists && (
            <OverwriteImpact preview={preview} triggers={triggers} />
          )}
          {tab.dirty && (
            <p className="text-fg-3" data-testid="overwrite-default-saves-first">
              Your unsaved changes are saved for this run first.
            </p>
          )}
          {error && (
            <p className="text-st-failed" data-testid="overwrite-default-error">{error}</p>
          )}
        </div>
        <div className="mt-4 flex shrink-0 justify-end gap-2">
          <button
            onClick={onClose}
            disabled={busy}
            data-testid="overwrite-default-cancel"
            className="cursor-pointer rounded-md border border-line-strong bg-bg-3 px-3 py-1.5 text-fg-2 transition-colors hover:bg-bg-4 disabled:opacity-40"
            style={{ fontSize: "11.5px" }}
          >
            Cancel
          </button>
          <button
            onClick={() => void confirm()}
            disabled={!canConfirm}
            data-testid="overwrite-default-confirm"
            className="cursor-pointer rounded-md bg-st-failed px-3 py-1.5 font-medium text-white transition-colors hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-40"
            style={{ fontSize: "11.5px" }}
          >
            {busy ? "Overwriting…" : "Overwrite default pipeline"}
          </button>
        </div>
      </div>
    </div>
  );
}

/** Who the overwrite touches: the Triggers, and a change since the launch. */
function OverwriteImpact({
  preview,
  triggers,
}: {
  preview: OverwritePreview;
  triggers: OverwritePreview["triggers"];
}) {
  return (
    <>
      <div data-testid="overwrite-default-triggers">
        {triggers.length === 0 ? (
          <p>No trigger launches this pipeline.</p>
        ) : (
          <>
            <p>
              {triggers.length === 1
                ? "1 trigger launches this pipeline and will use the new version:"
                : `${triggers.length} triggers launch this pipeline and will use the new version:`}
            </p>
            <ul className="mt-1 flex flex-col gap-0.5 pl-3">
              {triggers.map((t) => (
                <li key={t.id} className="list-disc text-fg" data-testid="overwrite-default-trigger">
                  {t.name}
                  {!t.enabled && <span className="text-fg-4"> (disabled)</span>}
                </li>
              ))}
            </ul>
          </>
        )}
      </div>
      {preview.modified_since_launch === true && (
        <p
          className="flex items-start gap-1.5 rounded border border-st-await/40 bg-st-await/10 px-2 py-1.5 text-st-await"
          data-testid="overwrite-default-modified"
        >
          <TriangleAlert size={12} className="mt-[2px] shrink-0" />
          <span>
            The default pipeline was changed since this run was launched. Overwriting replaces those
            changes.
          </span>
        </p>
      )}
      {preview.modified_since_launch == null && (
        <p className="text-fg-4" data-testid="overwrite-default-modified-unknown">
          PDO cannot tell whether the default pipeline changed since this run was launched.
        </p>
      )}
    </>
  );
}
