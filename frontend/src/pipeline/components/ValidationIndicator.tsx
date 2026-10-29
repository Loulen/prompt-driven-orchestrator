import { FlaskConical, Folders, Globe } from "lucide-react";
import type { PipelineValidation } from "../../shared/generated/PipelineValidation";
import type { Project } from "../../workspace/model/project";
import { Tooltip } from "../../shared/ui/kit";
import { validationSummary, validationTooltip } from "../model/pipelineValidation";

/**
 * A Pipeline's validation at a glance (CONTEXT.md « Pipeline validé »), on a row of
 * the Pipelines tab and in the tab bar of an open Pipeline: a globe and « All » for
 * every Projet, the folders glyph and the Projet count otherwise, a flask and 0 for
 * a test Pipeline. Hovering lists the Projets. Never a « validated » badge — the
 * launch form says it by its sections.
 */
export default function ValidationIndicator({
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
  const state = summary.all ? "all" : summary.count > 0 ? "projects" : "none";
  return (
    <Tooltip label={tooltip}>
      <span
        data-testid={testId}
        data-validation={state}
        data-count={summary.all ? undefined : summary.count}
        aria-label={tooltip}
        role="img"
        className="lp-valid"
      >
        <Icon size={11} strokeWidth={1.75} aria-hidden />
        <span>{summary.all ? "All" : summary.count}</span>
      </span>
    </Tooltip>
  );
}
