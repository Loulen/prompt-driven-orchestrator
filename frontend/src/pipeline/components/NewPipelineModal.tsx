/**
 * Create a pipeline: its name, and the editor opens on it.
 */

import { useState } from "react";
import { ApiError } from "../../shared/http/client";
import { createPipeline } from "../api/pipelines";
import { useEditStore } from "../store/editStore";

export default function NewPipelineModal({ onClose }: { onClose: () => void }) {
  const [name, setName] = useState("");
  // #825 (FP iteration 2): what the daemon said when it refused. This used to be
  // swallowed, and a refused Create was a button that did nothing — no message,
  // no error state, the dialog simply sitting there. The way to meet it is
  // ordinary: replay the *First pipeline* tour on an instance where you kept the
  // pipeline it builds, and every click on Create is a silent 409.
  const [error, setError] = useState<string | null>(null);

  const loadPipelines = useEditStore((s) => s.loadPipelines);
  const openPipeline = useEditStore((s) => s.openPipeline);

  async function handleCreate() {
    if (!name.trim()) return;
    setError(null);
    try {
      const result = await createPipeline(name.trim());
      await loadPipelines();
      await openPipeline(result.id);
      onClose();
    } catch (e) {
      // The collision is the one a reader can act on, so it is said in their
      // terms — « pipeline already exists » does not name the pipeline. Anything
      // else is quoted as the daemon phrased it rather than paraphrased.
      setError(
        e instanceof ApiError && e.status === 409
          ? `A pipeline named ${name.trim()} already exists. Open it from the list, or pick another name.`
          : e instanceof Error && e.message
            ? e.message
            : "The pipeline could not be created.",
      );
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-scrim">
      <div
        // #823: the *First pipeline* tour watches for the dialog itself (its
        // presence ends the « create a new pipeline » step); the naming step
        // lights only the field, so Create stays under the dim until asked (#837).
        data-testid="new-pipeline-dialog"
        className="w-[360px] rounded-lg border border-line bg-surface-2 p-4 text-sm"
      >
        <div className="mb-3 font-medium text-text">New Pipeline</div>

        <label className="mb-1 block text-text-3 text-xs">
          Name
        </label>
        <input
          value={name}
          onChange={(e) => {
            setName(e.target.value);
            // The refusal was about the name that was in the field; editing it
            // is the user answering, so the sentence stops applying.
            setError(null);
          }}
          placeholder="my-pipeline"
          data-testid="new-pipeline-name"
          className="mb-3 w-full rounded border border-line-strong bg-surface-2 px-2 py-1.5 text-text outline-none focus:border-primary"
          autoFocus
          onKeyDown={(e) => e.key === "Enter" && handleCreate()}
        />

        {error && (
          <div
            data-testid="new-pipeline-error"
            role="alert"
            className="rounded border border-st-failed/40 bg-st-failed/10 px-2 py-1.5 text-st-failed text-xs"
          >
            {error}
          </div>
        )}

        <div className="mt-4 flex justify-end gap-2">
          <button
            onClick={onClose}
            className="rounded border border-line-strong bg-surface-2 px-3 py-1 text-text-3 transition-colors hover:text-text"
          >
            Cancel
          </button>
          <button
            onClick={handleCreate}
            data-testid="new-pipeline-create"
            disabled={!name.trim()}
            className="rounded bg-primary px-3 py-1 font-medium text-surface-0 transition-colors hover:bg-primary-hi disabled:opacity-50"
          >
            Create
          </button>
        </div>
      </div>
    </div>
  );
}
