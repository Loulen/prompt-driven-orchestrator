import { useEffect, useState } from "react";
import { Globe } from "lucide-react";
import type { PipelineValidation } from "../../shared/generated/PipelineValidation";
import type { Project } from "../../workspace/model/project";
import { setPipelineValidation } from "../api/pipelines";
import { projectIdsOf, validationFromChoice } from "../model/pipelineValidation";

/**
 * The Validate modal (CONTEXT.md « Pipeline validé »): Projets to tick, plus « All
 * projects (current and future) ». Under « All projects » the boxes stay live and
 * list the Pipeline first for the Projets ticked; otherwise they are the only Projets
 * it is offered to, and unticking everything makes it a test Pipeline again. Saves on
 * its own: the validation is data of the instance, not part of the YAML the Save
 * button writes.
 */
export default function ValidatePipelineModal({
  open,
  pipelineId,
  pipelineName,
  validation,
  projects,
  onClose,
  onSaved,
}: {
  open: boolean;
  pipelineId: string;
  pipelineName: string;
  validation: PipelineValidation;
  projects: Project[];
  onClose: () => void;
  onSaved: (validation: PipelineValidation) => void;
}) {
  const [all, setAll] = useState(false);
  const [ticked, setTicked] = useState<string[]>([]);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Seeded on every open from what is stored, so a cancelled edit never lingers.
  useEffect(() => {
    if (!open) return;
    /* eslint-disable react-hooks/set-state-in-effect -- one-shot seed per open */
    setAll(validation.kind === "all");
    setTicked(projectIdsOf(validation));
    setError(null);
    setSaving(false);
    /* eslint-enable react-hooks/set-state-in-effect */
  }, [open, validation]);

  useEffect(() => {
    if (!open) return;
    function handleKey(e: KeyboardEvent) {
      if (e.key === "Escape") onClose();
    }
    document.addEventListener("keydown", handleKey);
    return () => document.removeEventListener("keydown", handleKey);
  }, [open, onClose]);

  if (!open) return null;

  const toggle = (id: string) =>
    setTicked((prev) => (prev.includes(id) ? prev.filter((x) => x !== id) : [...prev, id]));

  // The modal only shows the Projets the actor can read. The stored ones it
  // cannot see are kept as they are: saving must not withdraw a Projet nobody
  // here could untick.
  const visible = (id: string) => projects.some((p) => p.id === id);
  const hidden = projectIdsOf(validation).filter((id) => !visible(id));
  const chosen = validationFromChoice(all, [...ticked.filter(visible), ...hidden]);

  const save = async () => {
    setSaving(true);
    setError(null);
    try {
      const res = await setPipelineValidation(pipelineId, chosen);
      onSaved(res.validation);
      onClose();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setSaving(false);
    }
  };

  // Named in the stored order; the Projets the actor cannot see are counted.
  const firstFor =
    chosen.kind === "all"
      ? [
          ...ticked.flatMap((id) => projects.find((p) => p.id === id)?.name ?? []),
          ...(hidden.length > 0 ? [`${hidden.length} hidden`] : []),
        ]
      : [];
  const verdict =
    chosen.kind === "all"
      ? firstFor.length > 0
        ? `Offered to every project, listed first for ${firstFor.join(", ")}.`
        : "Offered to every project, including the ones created later."
      : chosen.kind === "none"
        ? "Nothing ticked: a test pipeline, offered only behind « Show test pipelines »."
        : `Offered to ${chosen.project_ids.length} project${chosen.project_ids.length > 1 ? "s" : ""}.`;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-scrim"
      data-testid="validate-pipeline-backdrop"
      onClick={onClose}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="validate-pipeline-title"
        data-testid="validate-pipeline-modal"
        className="flex max-h-[calc(85vh/var(--app-zoom,1))] w-[400px] max-w-[calc(90vw/var(--app-zoom,1))] flex-col rounded-lg border border-line bg-surface-1 p-4 shadow-lg"
        style={{ fontSize: "12px" }}
        onClick={(e) => e.stopPropagation()}
      >
        <h3 id="validate-pipeline-title" className="shrink-0 font-medium text-text" style={{ fontSize: "13px" }}>
          Validate <span className="font-mono">{pipelineName}</span>
        </h3>
        <p className="mt-1 shrink-0 text-text-3">
          Choose the projects whose New Run and Trigger forms offer this pipeline, or list it
          first.
        </p>

        <div className="mt-3 min-h-0 overflow-y-auto rounded-md border border-line bg-surface-0 p-1">
          <label
            className="flex cursor-pointer items-center gap-2 rounded px-2 py-1.5 text-text hover:bg-hover"
            data-testid="validate-all-row"
          >
            <input
              type="checkbox"
              checked={all}
              onChange={(e) => setAll(e.target.checked)}
              data-testid="validate-all"
              className="accent-primary"
            />
            <Globe size={12} className="shrink-0 text-text-3" aria-hidden />
            <span>All projects (current and future)</span>
          </label>
          <div className="my-1 h-px bg-line" />
          <p
            className="px-2 pt-1 pb-0.5 uppercase tracking-wider text-text-3"
            data-testid="validate-projects-heading"
            style={{ fontSize: "9.5px" }}
          >
            {all ? "Listed first for" : "Only offered to"}
            {all && (
              <span className="normal-case tracking-normal"> · already offered to every project</span>
            )}
          </p>
          {projects.length === 0 ? (
            <p className="px-2 py-1.5 text-text-3" data-testid="validate-no-project" style={{ fontSize: "11px" }}>
              No project yet. Name a repository group in the Runs list to create one.
            </p>
          ) : (
            projects.map((project) => (
              <label
                key={project.id}
                className="flex cursor-pointer items-center gap-2 rounded px-2 py-1.5 text-text-2 hover:bg-hover"
              >
                <input
                  type="checkbox"
                  // Under « All », a ticked box lists the Pipeline first for it.
                  checked={ticked.includes(project.id)}
                  onChange={() => toggle(project.id)}
                  data-testid={`validate-project-${project.id}`}
                  className="accent-primary"
                />
                <span className="truncate">{project.name}</span>
                <span className="ml-auto shrink-0 text-text-3" style={{ fontSize: "10px" }}>
                  {project.members.length} repo{project.members.length === 1 ? "" : "s"}
                </span>
              </label>
            ))
          )}
        </div>

        <p className="mt-2 shrink-0 text-text-3" data-testid="validate-verdict" style={{ fontSize: "11px" }}>
          {verdict}
        </p>
        {error && (
          <p className="mt-2 shrink-0 text-st-failed" data-testid="validate-error" style={{ fontSize: "11px" }}>
            {error}
          </p>
        )}

        <div className="mt-4 flex shrink-0 justify-end gap-2">
          <button
            onClick={onClose}
            data-testid="validate-cancel"
            className="cursor-pointer rounded-md border border-line-strong bg-surface-2 px-3 py-1.5 text-text-2 transition-colors hover:bg-hover"
            style={{ fontSize: "11.5px" }}
          >
            Cancel
          </button>
          <button
            onClick={save}
            disabled={saving}
            data-testid="validate-save"
            className="cursor-pointer rounded-md bg-primary px-3 py-1.5 font-medium text-primary-ink transition-colors hover:bg-primary-hi disabled:opacity-40"
            style={{ fontSize: "11.5px" }}
          >
            {saving ? "Saving…" : "Save validation"}
          </button>
        </div>
      </div>
    </div>
  );
}
