import { useEffect, useState } from "react";
import { FlaskConical, Folders, Globe } from "lucide-react";
import { ApiError, setPipelineValidation } from "../api";
import type { PipelineValidation, Project } from "../types";
import {
  namedProjectIds,
  validationFromChoice,
  validationLabel,
  validationSummary,
  validationTooltip,
} from "../lib/pipelineValidation";
import { Tooltip, TooltipProvider } from "./ui/tooltip";

/**
 * Pipelines validés par Projet (#974, CONTEXT.md « Pipeline validé ») — the two
 * surfaces that show and change a Pipeline's validation: the indicator (a row of the
 * Pipelines tab, the tab bar of an open Pipeline) and the Validate modal (next to
 * Save). One concern, one sibling; the reading rules live in lib/pipelineValidation.
 */

/**
 * Icon + number: a globe and « All » for every Projet (« All +N » when also listed
 * first for N Projets, #992), the folders glyph and the Projet count otherwise, a
 * flask and 0 for a test Pipeline. Hovering lists the
 * Projets. Never a « validated » badge — the launch form says it by its sections.
 */
export function ValidationIndicator({
  validation,
  projects,
  testId = "validation-indicator",
}: {
  validation: PipelineValidation;
  projects: Project[];
  testId?: string;
}) {
  const summary = validationSummary(validation, projects);
  const tooltip = validationTooltip(summary);
  const Icon = summary.all ? Globe : summary.count > 0 ? Folders : FlaskConical;
  // Its own provider: the indicator sits in rows and bars that are rendered on
  // their own too (tests, the tab bar), and a Radix tooltip needs one above it.
  return (
    <TooltipProvider>
      <Tooltip content={tooltip}>
        <span
          data-testid={testId}
          data-validation={summary.all ? "all" : summary.count > 0 ? "projects" : "none"}
          data-count={summary.all && summary.count === 0 ? undefined : summary.count}
          aria-label={tooltip}
          role="img"
          className={`inline-flex shrink-0 items-center gap-0.5 font-sans ${
            summary.all || summary.count > 0 ? "text-fg-3" : "text-fg-5"
          }`}
          style={{ fontSize: "10px" }}
        >
          <Icon size={11} aria-hidden />
          <span>{validationLabel(summary)}</span>
        </span>
      </Tooltip>
    </TooltipProvider>
  );
}

/**
 * The Validate modal: Projets to tick, plus « All projects (current and future) ».
 * Under « All », the ticked Projets are the ones it is listed first for (#992);
 * without it, the only ones it is offered to. The boxes stay active either way and
 * toggling « All » keeps them. Unticking everything makes the Pipeline a test
 * Pipeline again. Saves on its own
 * (the validation is instance data, not part of the YAML the Save button writes).
 */
export function ValidatePipelineModal({
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
    setTicked(namedProjectIds(validation));
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

  const chosen = validationFromChoice(
    all,
    // Only Projets that still exist: a stale id would be refused by the daemon.
    ticked.filter((id) => projects.some((p) => p.id === id)),
  );

  const save = async () => {
    setSaving(true);
    setError(null);
    try {
      const res = await setPipelineValidation(pipelineId, chosen);
      onSaved(res.validation);
      onClose();
    } catch (e) {
      setError(e instanceof ApiError || e instanceof Error ? e.message : String(e));
      setSaving(false);
    }
  };

  const firstFor = namedProjectIds(chosen)
    .map((id) => projects.find((p) => p.id === id)?.name)
    .filter((n): n is string => n != null);
  const verdict =
    chosen.kind === "all"
      ? firstFor.length > 0
        ? `Offered to every project, listed first for ${firstFor.join(", ")}.`
        : "Offered to every project, including the ones created later."
      : chosen.kind === "none"
        ? "Nothing ticked: a test pipeline, offered only behind « Show test pipelines »."
        : `Offered to ${chosen.project_ids.length} project${chosen.project_ids.length > 1 ? "s" : ""} only.`;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60"
      data-testid="validate-pipeline-backdrop"
      onClick={onClose}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="validate-pipeline-title"
        data-testid="validate-pipeline-modal"
        className="flex max-h-[85vh] w-[400px] max-w-[90vw] flex-col rounded-lg border border-line bg-bg-2 p-4 shadow-lg"
        style={{ fontSize: "12px" }}
        onClick={(e) => e.stopPropagation()}
      >
        <h3 id="validate-pipeline-title" className="shrink-0 font-medium text-fg" style={{ fontSize: "13px" }}>
          Validate <span className="font-mono">{pipelineName}</span>
        </h3>
        <p className="mt-1 shrink-0 text-fg-3">
          Choose the projects whose New Run and Trigger forms offer this pipeline, or list it first.
        </p>

        <div className="mt-3 min-h-0 overflow-y-auto rounded-md border border-line bg-bg-1 p-1">
          <label
            className="flex cursor-pointer items-center gap-2 rounded px-2 py-1.5 text-fg hover:bg-bg-3"
            data-testid="validate-all-row"
          >
            <input
              type="checkbox"
              checked={all}
              onChange={(e) => setAll(e.target.checked)}
              data-testid="validate-all"
              className="accent-acc"
            />
            <Globe size={12} className="shrink-0 text-fg-4" aria-hidden />
            <span>All projects (current and future)</span>
          </label>
          <div className="my-1 h-px bg-line" />
          <div
            className="flex items-baseline gap-1.5 px-2 pt-1 pb-0.5"
            data-testid="validate-projects-heading"
          >
            <span className="tracking-wider text-fg-4 uppercase" style={{ fontSize: "9.5px" }}>
              {all ? "Listed first for" : "Only offered to"}
            </span>
            <span className="text-fg-5" style={{ fontSize: "10px" }}>
              · {all ? "already offered to every project" : "hidden from every other project"}
            </span>
          </div>
          {projects.length === 0 ? (
            <p className="px-2 py-1.5 text-fg-4" data-testid="validate-no-project" style={{ fontSize: "11px" }}>
              No project yet. Name a repository group in the Runs list to create one.
            </p>
          ) : (
            projects.map((project) => (
              <label
                key={project.id}
                className="flex cursor-pointer items-center gap-2 rounded px-2 py-1.5 text-fg-2 hover:bg-bg-3"
              >
                <input
                  type="checkbox"
                  checked={ticked.includes(project.id)}
                  onChange={() => toggle(project.id)}
                  data-testid={`validate-project-${project.id}`}
                  className="accent-acc"
                />
                <span className="truncate">{project.name}</span>
                <span className="ml-auto shrink-0 text-fg-4" style={{ fontSize: "10px" }}>
                  {project.members.length} repo{project.members.length === 1 ? "" : "s"}
                </span>
              </label>
            ))
          )}
        </div>

        <p className="mt-2 shrink-0 text-fg-4" data-testid="validate-verdict" style={{ fontSize: "11px" }}>
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
            className="cursor-pointer rounded-md border border-line-strong bg-bg-3 px-3 py-1.5 text-fg-2 transition-colors hover:bg-bg-4"
            style={{ fontSize: "11.5px" }}
          >
            Cancel
          </button>
          <button
            onClick={save}
            disabled={saving}
            data-testid="validate-save"
            className="cursor-pointer rounded-md bg-acc px-3 py-1.5 font-medium text-on-acc transition-colors hover:bg-acc-dim disabled:opacity-40"
            style={{ fontSize: "11.5px" }}
          >
            {saving ? "Saving…" : "Save validation"}
          </button>
        </div>
      </div>
    </div>
  );
}
