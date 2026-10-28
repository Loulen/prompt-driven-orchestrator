import { FlaskConical, Folders, Globe } from "lucide-react";
import type { PipelineValidation } from "../../shared/generated/PipelineValidation";
import type { Project } from "../../workspace/model/project";
import { Tooltip } from "../../shared/ui/tooltip";
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
  // Its own provider: the indicator sits in rows and bars that are rendered on
  // their own too (tests), and a Radix tooltip needs one above it.
  return (
    <>
      <Tooltip content={tooltip}>
        <span
          data-testid={testId}
          data-validation={summary.all ? "all" : summary.count > 0 ? "projects" : "none"}
          data-count={summary.all ? undefined : summary.count}
          aria-label={tooltip}
          role="img"
          className={`inline-flex shrink-0 items-center gap-0.5 font-sans ${
            summary.all || summary.count > 0 ? "text-text-3" : "text-text-3"
          }`}
          style={{ fontSize: "10px" }}
        >
          <Icon size={11} aria-hidden />
          <span>{summary.all ? "All" : summary.count}</span>
        </span>
      </Tooltip>
    </>
  );
}
