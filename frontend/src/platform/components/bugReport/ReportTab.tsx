import { CircleAlert, Play, Workflow } from "lucide-react";
import { Checkbox, Select, TextArea, type SelectOption } from "../../../shared/ui/kit";
import type { PipelineListEntry } from "../../../shared/generated/PipelineListEntry";
import type { RunListEntry } from "../../../shared/generated/RunListEntry";
import type { BugReportDestination } from "../../model/bugReport";
import CaptureZone from "./CaptureZone";

/** What the Report tab edits. */
export interface ReportForm {
  description: string;
  includeRun: boolean;
  runId: string | null;
  includePipeline: boolean;
  pipelineId: string | null;
  includePrompts: boolean;
}

/** The menu's options, the current value kept when the list does not hold it (an archived Run). */
function withCurrent(options: SelectOption[], value: string | null): SelectOption[] {
  if (!value || options.some((o) => o.value === value)) return options;
  return [{ value, label: value }, ...options];
}

const runOptions = (runs: RunListEntry[], value: string | null) =>
  withCurrent(
    runs.map((r) => ({ value: r.run_id, label: `${r.run_id} · ${r.pipeline_name}`, hint: r.status })),
    value,
  );

const pipelineOptions = (pipelines: PipelineListEntry[], value: string | null) =>
  withCurrent(
    pipelines.map((p) => ({ value: p.id, label: p.name })),
    value,
  );

function Missing({ message, testId }: { message: string | undefined; testId: string }) {
  if (!message) return null;
  return (
    <span className="d-err" role="alert" data-testid={testId}>
      <CircleAlert size={12} strokeWidth={1.5} aria-hidden />
      {message}
    </span>
  );
}

/**
 * The Report tab: the description, the screenshot, then what the report joins
 * (a Run, a Pipeline, the prompts where the destination accepts them), each
 * checked box with its menu, in error while it has no choice.
 */
export default function ReportTab({
  form,
  change,
  runs,
  pipelines,
  capture,
  onCapture,
  missing,
  destination,
}: {
  form: ReportForm;
  change: (patch: Partial<ReportForm>) => void;
  runs: RunListEntry[];
  pipelines: PipelineListEntry[];
  capture: Blob | null;
  onCapture: (capture: Blob | null) => void;
  missing: { run?: string; pipeline?: string };
  destination: BugReportDestination;
}) {
  const promptsRefused = !destination.acceptsPrompts;
  return (
    <>
      <TextArea
        className="br-desc"
        aria-label="Description"
        placeholder="Describe the bug encountered, add a screenshot if possible"
        value={form.description}
        onChange={(e) => change({ description: e.target.value })}
        data-autofocus=""
        data-testid="bug-report-description"
      />
      <CaptureZone capture={capture} onChange={onCapture} />
      <div className="br-joins">
        <Checkbox
          label="Include run details"
          checked={form.includeRun}
          // Unchecked, the choice goes: checked again, it is asked anew.
          onChange={(on) => change(on ? { includeRun: true } : { includeRun: false, runId: null })}
        >
          Include run details
        </Checkbox>
        <div className="br-joins__ctl" data-testid="bug-report-run">
          <Select
            label="Run"
            placeholder="Pick a run"
            icon={Play}
            value={form.runId}
            options={runOptions(runs, form.runId)}
            onChange={(runId) => change({ includeRun: true, runId })}
            invalid={!!missing.run}
            disabled={!form.includeRun}
          />
          <Missing message={missing.run} testId="bug-report-run-error" />
        </div>
        <Checkbox
          label="Include pipeline details"
          checked={form.includePipeline}
          onChange={(on) => change(on ? { includePipeline: true } : { includePipeline: false, pipelineId: null })}
        >
          Include pipeline details
        </Checkbox>
        <div className="br-joins__ctl" data-testid="bug-report-pipeline">
          <Select
            label="Pipeline"
            placeholder="Pick a pipeline"
            icon={Workflow}
            value={form.pipelineId}
            options={pipelineOptions(pipelines, form.pipelineId)}
            onChange={(pipelineId) => change({ includePipeline: true, pipelineId })}
            invalid={!!missing.pipeline}
            disabled={!form.includePipeline}
          />
          <Missing message={missing.pipeline} testId="bug-report-pipeline-error" />
        </div>
        <span data-testid="bug-report-prompts">
          <Checkbox
            label="Include prompts"
            checked={form.includePrompts && !promptsRefused}
            disabled={promptsRefused || !form.includePipeline}
            onChange={(includePrompts) => change({ includePrompts })}
          >
            Include prompts
          </Checkbox>
        </span>
        <span className="br-joins__note">
          {!promptsRefused
            ? "Node prompts of the run and pipeline"
            : destination.extension
              ? "This destination does not accept prompts"
              : "Not sent to a public tracker"}
        </span>
      </div>
    </>
  );
}
